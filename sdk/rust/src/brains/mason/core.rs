//! Mason's capture core: Ridgeline's border-holding land racer (as forked into
//! Pangolin), with every capture priced by the exact v0.7 regions it seals off.
//!
//! On top of Ridgeline (`ring` knob on): each tick one Tarjan pass ([`super::rings`])
//! prices the regions every hex would seal off for us; a short lookahead re-runs it
//! after each of the most promising first steps to credit captures that set up a ring
//! next tick; and one pass per rival finds the necks through which a rival capture
//! would swallow our land, so we garrison those hexes or take the neutral ones first.
//!
//! Late in a free-for-all every seat is limited by its one action a tick, not by troops,
//! so land is decided by how many captured hexes stay captured. Ridgeline sizes every
//! border hex's garrison against the visible enemy stacks next to it, scaled by the
//! height they would attack uphill or downhill from, so high ground and hexes shielded
//! by mountains, water and the map edge are cheap to hold. Captures split the attacking
//! hex's troops so both it and the new hex stay out of reach, are only sent when they
//! can't plausibly bounce, and prefer ground that shortens the exposed border or makes an
//! enemy pocket surrender. A capture whose new hex would be left too thin to hold is not
//! made at all, and troops get cheaper the more of them we bank per hex. Spare actions
//! relay interior stacks toward short border hexes.

use super::rings::{Board, Loot, Tarjan};
use crate::{Action, Bot, Building, Hex, PlayerCommand, PlayerId, RulesConfig, Style, View, MILLI};
use std::collections::VecDeque;

/// Map-static data, built from the first view.
struct Geometry {
    radius: i32,
    width: i32,
    passable: Vec<bool>,
    height: Vec<i8>,
    /// Passable neighbour slots of each slot.
    nbrs: Vec<Vec<usize>>,
    hexes: Vec<Hex>,
}

impl Geometry {
    fn new(view: &View) -> Self {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let mut passable = vec![false; slots];
        let mut height = vec![0i8; slots];
        let mut hexes = vec![Hex::new(0, 0); slots];
        for i in 0..slots {
            hexes[i] = Hex::new(i as i32 % width - radius, i as i32 / width - radius);
            if let Some(t) = view.public_terrain[i] {
                passable[i] = t.passable();
                height[i] = t.height;
            }
        }
        let mut g = Geometry { radius, width, passable, height, nbrs: vec![], hexes };
        g.nbrs = (0..slots)
            .map(|i| {
                g.hexes[i]
                    .neighbors()
                    .iter()
                    .filter_map(|n| g.slot(*n))
                    .filter(|&j| g.passable[j])
                    .collect()
            })
            .collect();
        g
    }

    fn slot(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * self.width + h.q + r) as usize)
    }
}

#[derive(Clone, Copy)]
struct Cell {
    owner: Option<PlayerId>,
    troops: u32,
    wall_milli: u32,
    building: Option<Building>,
}

/// One affordable capture under consideration.
#[derive(Clone, Copy)]
struct Capture {
    score: i64,
    from: usize,
    to: usize,
    send: u32,
}

/// How the land we don't own hangs together this tick.
struct Seal {
    /// Hexes of small, enemy-free regions that taking this hex would cut off from the
    /// rest of the map behind our land, mountains, water and the map edge.
    cuts: Vec<u32>,
    /// Hexes already inside such a region: nobody else can reach them without first
    /// taking our land, so they can wait.
    pocket: Vec<bool>,
}

/// Tunables, read once from the genome's knobs.
struct Knobs {
    /// Standard deviations (x10) of bad dice every capture must survive.
    sure_x10: u64,
    land: i64,
    enemy: i64,
    hug: i64,
    open: i64,
    height: i64,
    cost_pct: i64,
    /// Troops per hex at which a troop's cost halves (0 = troops always cost full price).
    wealth: u64,
    enclose: i64,
    /// Garrison any enemy-facing hex wants even when no big stack is next to it.
    guard: u64,
    /// % of what the biggest adjacent enemy stack kills going all in that a border hex
    /// wants to hold.
    margin_pct: u64,
    /// Weight of the source hex's lost safety against the new hex's land.
    drop_w: i64,
    /// Captures leaving the new hex less safe (milli) than this are skipped.
    safe_min: i64,
    supply_every: u32,
    /// Score per hex of backyard a capture seals off.
    seal: i64,
    /// Largest region (hexes) that counts as a backyard.
    seal_max: u32,
    /// Score lost for taking a hex that is already in our backyard.
    backyard: i64,
    /// Over the last this many ticks, race for land that only has to last until the cutoff.
    sprint: u32,
    /// Safety gain (milli) a supply move must buy on its cadence tick.
    supply_min: i64,
    /// Captures scoring below `bar - 500` give the tick to supply or regrouping.
    min_score: i64,
    /// Gathers are allowed before this tick while no enemy touches our land.
    gather_until: u32,
    /// Score a capture we can't afford yet must beat the best affordable one by before
    /// the tick stages troops onto its launch hex instead (0 = never stage).
    stage: i64,
    /// Score lost per troop the launch hex is short of a blocked capture.
    short_cost: i64,
    /// From this tick (0 = never) the late knobs below replace their counterparts: once
    /// overtime odds favour attackers, held borders can't be made safe and only pressure
    /// ends the game.
    late_from: u32,
    late_margin_pct: u64,
    late_safe_min: i64,
    late_min_score: i64,
    late_sure_x10: u64,
    /// From `late_from` on, play the sprint (captures need only be sure, not safe) (0/1).
    late_sprint: bool,
    /// While sprinting, a capture sends at least this % of the pad's spare troops so the
    /// army rolls forward with it.
    send_pct: u64,
    /// Late enemy-hex bonus, replacing `enemy`.
    late_enemy: i64,
    /// Late `stage`, replacing it when above 0: walking the interior bank toward captures
    /// we can't afford yet pays once the lattice is built, not while it grows.
    late_stage: i64,
    /// From `late_from` on, Gather toward the cheapest target when no capture is ready (0/1).
    late_gather: bool,
    /// Once a single rival is left alive, turn on the late sprint and late Gathers with
    /// `blitz_send_pct` and `blitz_push` (0/1): with one rival, rolling forward breaks the
    /// overtime churn; with several, it feeds third parties.
    duel_blitz: bool,
    blitz_send_pct: u64,
    blitz_push: u32,
    /// Finishing mode, the blitz above plus `prey`, once at most this many players are
    /// alive, us included (0 = never).
    finish_alive: usize,
    /// ... or once we hold at least this % of the passable board (0 = never).
    finish_land_pct: u32,
    /// Either way, only while our land is at least this % of every alive rival's known
    /// land: a blitz that isn't leading feeds the bystander.
    finish_lead_pct: u32,
    /// What finishing mode turns on besides `prey`: 0 = nothing, 1 = the whole blitz,
    /// 2 = only its Gathers (mass the bank behind safe captures).
    finish_blitz: u32,
    /// With one rival, go late (and so blitz, with `duel_blitz`) from this tick instead of
    /// `late_from` (0 = off) ...
    duel_late_from: u32,
    /// ... once our land is at least this % of the rival's known land (0 = at once).
    duel_lead_pct: u32,
    /// In finishing mode, score per hex of the alive rival we know the least land of, so
    /// one rival is wiped out before the next.
    prey: i64,
    /// Extra score for that rival's Villages.
    prey_village: i64,
    /// Hunt only rivals that touch our land (0/1): fog makes a far rival look small.
    prey_border: bool,
    /// Tick the late phase began.
    late_at: Option<u32>,
    /// Bonus for an enemy hex of a rival we know `n` hexes of: `focus * 50 / n`, so the
    /// last hexes of a dying rival come first.
    focus: i64,
    /// In the late phase, every this many ticks (0 = never) Gather toward the best
    /// capture's launch hex, so one action brings the whole interior bank a step closer.
    push_gather: u32,
    /// Hexes we took in the last this many ticks count as fresh (0 = never reinforce);
    /// a move of a neighbour's spare troops into a fresh hex still short of its garrison
    /// takes the tick when it buys at least `reinforce_min` milli of safety.
    reinforce: u32,
    reinforce_min: i64,
    /// Before this tick (0 = never) the opening knobs below replace `margin_pct` and
    /// `safe_min`: on a crowded spawn every hex faces an enemy, and full garrisons freeze
    /// expansion while the rivals grow away from us.
    /// Go-style region capture (0/1; rules v0.7, which a frozen binary can't read): any
    /// region we don't own that our land closes off from the board edge becomes ours.
    region: bool,
    /// Largest region (hexes) we flood looking for one we'd close off.
    region_max: usize,
    /// Score for taking a rival's Village / Farm or Quarry with the hex.
    loot_village: i64,
    loot_farm: i64,
    open_until: u32,
    open_margin_pct: u64,
    open_safe_min: i64,
    base_margin_pct: u64,
    base_safe_min: i64,
    /// Price captures by the exact v0.7 regions they seal ([`super::rings`]) instead of
    /// `region`/`enclose`, and run the lookahead and neck defence below (0/1).
    ring: bool,
    /// Score per passable hex, rival hex, Village and Farm/Quarry a capture seals off,
    /// and % of the troops inside (they become ours).
    ring_hex: i64,
    ring_enemy: i64,
    ring_village: i64,
    ring_farm: i64,
    ring_troop_pct: i64,
    /// First steps the lookahead tries (0 = no lookahead), and % of the extra ring value
    /// the best second step would then seal that the first step is credited with.
    setup_k: usize,
    setup_pct: i64,
    /// Extra first steps on the board edge, next to the most rival land: cutting a
    /// rival's strip of edge is what makes its land enclosable.
    edge_k: usize,
    /// Second steps the lookahead tries after each first step (0 = two-step lookahead
    /// only), and % of the extra ring value a third step would then seal.
    setup3_k: usize,
    setup3_pct: i64,
    /// Score per hex of ours a rival would swallow by taking a hex we can take first.
    neck_take: i64,
    /// Extra % of garrison per hex of ours a rival would swallow by taking this one of ours.
    neck_want_pct: u64,
    /// Most swallowed hexes the neck terms count.
    neck_cap: u32,
    /// Extra hexes each swallowed Village counts as in the neck terms.
    neck_village: u32,
    /// A capture sealing at least this ring value takes the tick before supply and
    /// staging (0 = no priority).
    ring_first: i64,
    /// Walk troops toward the launch hex of an unaffordable ring worth at least this
    /// (0 = never).
    ring_stage: i64,
    /// In a multi-player lobby, from `wave_from` to `wave_until` (0 = off) whatever the
    /// phase, border hexes facing a rival whose known land is at least ours want
    /// `wave_pct`% more garrison, and supply runs every `wave_supply_every` ticks
    /// (0 = keep `supply_every`): most rivals go late around then and roll weak borders.
    wave_from: u32,
    wave_until: u32,
    wave_pct: u64,
    wave_supply_every: u32,
    /// Among sends that make a capture equally safe, send the fewest (0/1): a big stack
    /// rolled onto the border makes every rival facing it garrison up.
    lean: bool,
}

impl Knobs {
    fn read(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Knobs {
            sure_x10: k("sure_x10", 30) as u64,
            land: k("land", 100),
            enemy: k("enemy", 30),
            hug: k("hug", 40),
            open: k("open", 15),
            height: k("height", 10),
            cost_pct: k("cost_pct", 100),
            wealth: k("wealth", 30) as u64,
            enclose: k("enclose", 100),
            guard: k("guard", 20) as u64,
            margin_pct: k("margin_pct", 600) as u64,
            drop_w: k("drop_w", 100),
            safe_min: k("safe_min", 300),
            supply_every: k("supply_every", 3) as u32,
            seal: k("seal", 0),
            seal_max: k("seal_max", 40) as u32,
            backyard: k("backyard", 0),
            sprint: k("sprint", 0) as u32,
            supply_min: k("supply_min", 200),
            min_score: k("bar", 500) - 500,
            gather_until: k("gather_until", 600) as u32,
            stage: k("stage", 0),
            short_cost: k("short_cost", 0),
            late_from: k("late_from", 0) as u32,
            late_margin_pct: k("late_margin_pct", 100) as u64,
            late_safe_min: k("late_safe_min", 0),
            late_min_score: k("late_bar", 500) - 500,
            late_sure_x10: k("late_sure_x10", 20) as u64,
            late_sprint: k("late_sprint", 0) > 0,
            send_pct: k("send_pct", 0) as u64,
            late_enemy: k("late_enemy", 30),
            late_stage: k("late_stage", 0),
            late_gather: k("late_gather", 0) > 0,
            duel_blitz: k("duel_blitz", 0) > 0,
            blitz_send_pct: k("blitz_send_pct", 60) as u64,
            blitz_push: k("blitz_push", 10) as u32,
            finish_alive: k("finish_alive", 0) as usize,
            finish_land_pct: k("finish_land_pct", 0) as u32,
            finish_lead_pct: k("finish_lead_pct", 0) as u32,
            finish_blitz: k("finish_blitz", 1) as u32,
            duel_late_from: k("duel_late_from", 0) as u32,
            duel_lead_pct: k("duel_lead_pct", 0) as u32,
            prey: k("prey", 0),
            prey_village: k("prey_village", 0),
            prey_border: k("prey_border", 0) > 0,
            late_at: None,
            focus: k("focus", 0),
            push_gather: k("push_gather", 0) as u32,
            reinforce: k("reinforce", 0) as u32,
            reinforce_min: k("reinforce_min", 300),
            region: k("region", 0) > 0,
            region_max: k("region_max", 200) as usize,
            loot_village: k("loot_village", 0),
            loot_farm: k("loot_farm", 0),
            open_until: k("open_until", 0) as u32,
            open_margin_pct: k("open_margin_pct", 200) as u64,
            open_safe_min: k("open_safe_min", 0),
            base_margin_pct: k("margin_pct", 600) as u64,
            base_safe_min: k("safe_min", 300),
            ring: k("ring", 0) > 0,
            ring_hex: k("ring_hex", 100),
            ring_enemy: k("ring_enemy", 30),
            ring_village: k("ring_village", 150),
            ring_farm: k("ring_farm", 40),
            ring_troop_pct: k("ring_troop_pct", 50),
            setup_k: k("setup_k", 10) as usize,
            edge_k: k("edge_k", 0) as usize,
            setup_pct: k("setup_pct", 40),
            setup3_k: k("setup3_k", 0) as usize,
            setup3_pct: k("setup3_pct", 25),
            neck_take: k("neck_take", 60),
            neck_want_pct: k("neck_want_pct", 15) as u64,
            neck_cap: k("neck_cap", 30) as u32,
            neck_village: k("neck_village", 0) as u32,
            ring_first: k("ring_first", 300),
            ring_stage: k("ring_stage", 500),
            wave_from: k("wave_from", 0) as u32,
            wave_until: k("wave_until", 0) as u32,
            wave_pct: k("wave_pct", 0) as u64,
            wave_supply_every: k("wave_supply_every", 0) as u32,
            lean: k("lean", 0) > 0,
        }
    }

    fn opening(&mut self, tick: u32) {
        if tick < self.open_until {
            self.margin_pct = self.open_margin_pct;
            self.safe_min = self.open_safe_min;
        } else {
            self.margin_pct = self.base_margin_pct;
            self.safe_min = self.base_safe_min;
            self.open_until = 0;
        }
    }

    fn go_late(&mut self) {
        self.margin_pct = self.late_margin_pct;
        self.safe_min = self.late_safe_min;
        self.min_score = self.late_min_score;
        self.sure_x10 = self.late_sure_x10;
        self.enemy = self.late_enemy;
        if self.late_stage > 0 {
            self.stage = self.late_stage;
        }
        self.late_at = Some(self.late_from);
        self.late_from = 0;
    }
}

pub struct Core {
    /// Knobs for a duel: every `duel_X` knob of the genome replaces `X`. They take over
    /// in a 1v1, and in a bigger lobby once at most `ffa_duel_when` players are alive or
    /// we own at least `ffa_duel_share`% of the passable hexes (0 = off).
    duel: Option<Knobs>,
    ffa_duel_when: usize,
    ffa_duel_share: usize,
    /// Knobs for a multi-player endgame: every `end_X` knob replaces `X` once at most
    /// `ffa_end_when` players are alive in a bigger lobby (until the duel knobs take over).
    end: Option<Knobs>,
    ffa_end_when: usize,
    /// We held `ffa_duel_share` of the board last tick.
    dominant: bool,
    name: String,
    rules: RulesConfig,
    /// Chance (milli) an attacker wins an exchange this tick; rises in overtime.
    win: u32,
    k: Knobs,
    geo: Option<Geometry>,
    /// Last seen state of every slot.
    seen: Vec<Option<Cell>>,
    /// Hexes we last saw each rival own.
    land_of: Vec<u32>,
    /// In finishing mode (see `finish_alive`).
    finishing: bool,
    /// The rival finishing mode hunts this tick.
    prey_of: Option<PlayerId>,
    /// Late sprint, late Gather, send % and push gap to restore when finishing stops.
    calm_late: Option<(bool, bool, u64, u32)>,
    board: Option<Board>,
    tarjan: Tarjan,
    /// Hexes we own this tick.
    mine_now: Vec<bool>,
    /// Tick each hex last became ours (0 = never or at the start).
    flipped: Vec<u32>,
    /// What taking each hex would seal off for us this tick.
    ring: Vec<Loot>,
    /// Lookahead credit for taking each hex.
    setup: Vec<i64>,
    /// Most of our hexes one rival capture of each hex would swallow.
    neck: Vec<u32>,
    weights: Vec<Loot>,
    scratch: Vec<Loot>,
    blocked: Vec<bool>,
    scratch2: Vec<Loot>,
    blocked2: Vec<bool>,
    /// Per capture target, which `geo.nbrs` entries (bits) taking it seals off.
    sealed_mask: Vec<u8>,
    /// The capture this tick's action makes and the ring value it seals, if any.
    pub chosen: Option<(Hex, i64)>,
    /// Rivals (by id) the defensive wave window guards against this tick.
    wave: [bool; 16],
}

impl Core {
    /// The genome's knobs with every `PREFIX_X` knob replacing `X`, if it has any.
    fn overridden(style: &Style, prefix: &str) -> Option<Knobs> {
        let overrides: Vec<(String, i64)> = style
            .params
            .knobs
            .iter()
            .filter(|(n, _)| !matches!(n.as_str(), "duel_blitz" | "duel_late_from" | "duel_lead_pct"))
            .filter_map(|(n, v)| n.strip_prefix(prefix).map(|b| (b.to_string(), *v)))
            .collect();
        if overrides.is_empty() {
            return None;
        }
        let mut s = style.clone();
        s.params.knobs.extend(overrides);
        Some(Knobs::read(&s))
    }

    pub fn new(style: Style) -> Self {
        Core {
            duel: Self::overridden(&style, "duel_"),
            end: Self::overridden(&style, "end_"),
            ffa_end_when: style.params.knob("ffa_end_when", 0).max(0) as usize,
            ffa_duel_when: style.params.knob("ffa_duel_when", 0).max(0) as usize,
            ffa_duel_share: style.params.knob("ffa_duel_share", 0).max(0) as usize,
            dominant: false,
            name: style.id.clone(),
            rules: RulesConfig::default(),
            win: RulesConfig::default().attacker_exchange_win_milli,
            k: Knobs::read(&style),
            geo: None,
            seen: vec![],
            land_of: vec![],
            finishing: false,
            prey_of: None,
            calm_late: None,
            board: None,
            tarjan: Tarjan::default(),
            mine_now: vec![],
            flipped: vec![],
            ring: vec![],
            setup: vec![],
            neck: vec![],
            weights: vec![],
            scratch: vec![],
            blocked: vec![],
            scratch2: vec![],
            blocked2: vec![],
            sealed_mask: vec![],
            chosen: None,
            wave: [false; 16],
        }
    }

    /// Our hexes a rival would swallow by taking `h`, as of this tick.
    pub fn neck_at(&self, h: Hex) -> u32 {
        self.geo.as_ref().and_then(|g| g.slot(h)).and_then(|i| self.neck.get(i).copied()).unwrap_or(0)
    }

    fn ring_value(&self, l: &Loot) -> i64 {
        if l.hexes == 0 {
            return 0;
        }
        self.k.ring_hex * l.hexes as i64
            + self.k.ring_enemy * l.enemy as i64
            + self.k.ring_village * l.villages as i64
            + self.k.ring_farm * l.farms as i64
            + self.k.ring_troop_pct * l.troops as i64 / 100
    }

    /// Per-hex loot weights as we last saw the board, from `me`'s point of view.
    fn fill_weights(&mut self, b: &Board, me: PlayerId) {
        self.weights.clear();
        self.weights.resize(b.slots, Loot::default());
        for i in 0..b.slots {
            if !b.valid[i] {
                continue;
            }
            let w = &mut self.weights[i];
            w.nodes = 1;
            if !b.passable[i] {
                continue;
            }
            w.hexes = 1;
            if let Some(c) = self.seen[i] {
                w.troops = c.troops;
                match c.owner {
                    Some(o) if o == me => w.mine = 1,
                    Some(_) => w.enemy = 1,
                    None => {}
                }
                match c.building {
                    Some(Building::Village) => w.villages = 1,
                    Some(Building::Farm | Building::Quarry) => w.farms = 1,
                    _ => {}
                }
            }
        }
    }

    /// This tick's ring prices, lookahead credits and necks.
    fn analyse(&mut self, geo: &Geometry, me: PlayerId) {
        let b = self.board.take().unwrap();
        let slots = b.slots;
        self.fill_weights(&b, me);
        let mut ring = std::mem::take(&mut self.ring);
        self.tarjan.run(&b, &self.mine_now, &self.weights, &mut ring);
        self.ring = ring;
        self.setup.clear();
        self.setup.resize(slots, 0);
        let frontier: Vec<usize> = (0..slots)
            .filter(|&i| b.passable[i] && !self.mine_now[i] && geo.nbrs[i].iter().any(|&n| self.mine_now[n]))
            .collect();
        if self.k.setup_k > 0 && self.k.setup_pct > 0 {
            // How deep in a bay of `land` hex `a` sits: our hexes within two steps.
            let bay = |a: usize, land: &[bool]| {
                let h = geo.hexes[a];
                let mut n = 0u32;
                for x in h.neighbors() {
                    for y in x.neighbors() {
                        if y != h && geo.slot(y).is_some_and(|j| land[j]) {
                            n += 1;
                        }
                    }
                }
                n
            };
            // First steps worth trying: frontier hexes in the deepest bays of our land.
            let mut cands: Vec<(u32, usize)> = frontier
                .iter()
                .filter(|&&a| self.ring[a].hexes == 0)
                .map(|&a| (bay(a, &self.mine_now), a))
                .collect();
            cands.sort_unstable_by(|x, y| y.cmp(x));
            cands.truncate(self.k.setup_k);
            if self.k.edge_k > 0 {
                let rival = |j: usize| self.seen[j].is_some_and(|c| c.owner.is_some_and(|o| o != me));
                let near_rival = |a: usize| {
                    let h = geo.hexes[a];
                    h.neighbors()
                        .into_iter()
                        .flat_map(|x| x.neighbors())
                        .filter(|&y| y != h && geo.slot(y).is_some_and(rival))
                        .count() as u32
                };
                let mut edge: Vec<(u32, usize)> = frontier
                    .iter()
                    .filter(|&&a| b.boundary[a] && self.ring[a].hexes == 0 && !cands.iter().any(|c| c.1 == a))
                    .map(|&a| (near_rival(a), a))
                    .filter(|e| e.0 > 0)
                    .collect();
                edge.sort_unstable_by(|x, y| y.cmp(x));
                edge.truncate(self.k.edge_k);
                cands.extend(edge);
            }
            let mut after = std::mem::take(&mut self.scratch);
            let mut after2 = std::mem::take(&mut self.scratch2);
            let mut blocked2 = std::mem::take(&mut self.blocked2);
            for &(_, a) in &cands {
                self.blocked.clone_from(&self.mine_now);
                self.blocked[a] = true;
                self.tarjan.run(&b, &self.blocked, &self.weights, &mut after);
                let mut next: Vec<usize> = frontier.iter().copied().filter(|&j| j != a).collect();
                next.extend(geo.nbrs[a].iter().copied().filter(|&j| !self.blocked[j] && !frontier.contains(&j)));
                let v2 = next
                    .iter()
                    .map(|&j| self.ring_value(&after[j]) - self.ring_value(&self.ring[j]))
                    .max()
                    .unwrap_or(0);
                let mut best = v2 * self.k.setup_pct / 100;
                if self.k.setup3_k > 0 {
                    let mut c2: Vec<(u32, usize)> = next
                        .iter()
                        .filter(|&&j| after[j].hexes == 0)
                        .map(|&j| (bay(j, &self.blocked), j))
                        .collect();
                    c2.sort_unstable_by(|x, y| y.cmp(x));
                    c2.truncate(self.k.setup3_k);
                    for &(_, b2) in &c2 {
                        blocked2.clone_from(&self.blocked);
                        blocked2[b2] = true;
                        self.tarjan.run(&b, &blocked2, &self.weights, &mut after2);
                        let gain = |j: usize| self.ring_value(&after2[j]) - self.ring_value(&after[j]);
                        let v3 = next
                            .iter()
                            .copied()
                            .filter(|&j| j != b2)
                            .chain(geo.nbrs[b2].iter().copied().filter(|&j| !blocked2[j]))
                            .map(gain)
                            .max()
                            .unwrap_or(0);
                        best = best.max(v3 * self.k.setup3_pct / 100);
                    }
                }
                self.setup[a] = best;
            }
            self.scratch = after;
            self.scratch2 = after2;
            self.blocked2 = blocked2;
        }
        self.neck.clear();
        self.neck.resize(slots, 0);
        if self.k.neck_take > 0 || self.k.neck_want_pct > 0 {
            let mut rivals: Vec<PlayerId> =
                self.seen.iter().flatten().filter_map(|c| c.owner).filter(|&o| o != me).collect();
            rivals.sort_unstable();
            rivals.dedup();
            let mut out = std::mem::take(&mut self.scratch);
            for r in rivals {
                self.blocked.clear();
                self.blocked.extend((0..slots).map(|i| self.seen[i].is_some_and(|c| c.owner == Some(r))));
                self.tarjan.run(&b, &self.blocked, &self.weights, &mut out);
                for i in 0..slots {
                    if out[i].mine > 0 && geo.nbrs[i].iter().any(|&n| self.blocked[n]) {
                        let lost = out[i].mine + self.k.neck_village * out[i].villages;
                        self.neck[i] = self.neck[i].max(lost.min(self.k.neck_cap));
                    }
                }
            }
            self.scratch = out;
        }
        self.board = Some(b);
    }

    /// The most valuable ring worth more than `ring_now` that no capture this tick
    /// takes: launch it if its pad can afford it after all, else walk troops to the pad.
    fn ring_stage(&mut self, geo: &Geometry, me: PlayerId, mine: &[usize], ring_now: i64) -> Option<Action> {
        if !self.k.ring || self.k.ring_stage == 0 || mine.len() < 2 {
            return None;
        }
        let mut best: Option<(i64, usize, usize)> = None;
        for t in 0..geo.passable.len() {
            if self.ring[t].hexes == 0 || !geo.passable[t] || self.mine_now[t] {
                continue;
            }
            let v = self.ring_value(&self.ring[t]);
            if v < self.k.ring_stage || v <= ring_now {
                continue;
            }
            let Some(pad) = geo.nbrs[t]
                .iter()
                .copied()
                .filter(|&n| self.mine_now[n])
                .max_by_key(|&n| (self.seen[n].unwrap().troops, std::cmp::Reverse(n)))
            else {
                continue;
            };
            if best.is_none_or(|b| (v, std::cmp::Reverse(t)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((v, t, pad));
            }
        }
        let (v, t, pad) = best?;
        let tc = self.seen[t]?;
        let troops = self.seen[pad].unwrap().troops as u64;
        let defense = self.defense(geo, pad, t, tc.wall_milli) as u32;
        let (mean, sd) = self.loss_stats(tc.troops, defense);
        let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
        if troops >= need + 1 {
            if v < self.k.ring_first {
                return None;
            }
            let send = need + (troops - 1 - need) / 2;
            self.chosen = Some((geo.hexes[t], v));
            return Some(Action::Move { from: geo.hexes[pad], to: geo.hexes[t], troops: send as u32 });
        }
        let contact = mine.iter().any(|&s| geo.nbrs[s].iter().any(|&n| self.enemy(n, me)));
        self.stage_to(geo, me, pad, !contact)
    }

    /// Whether our neighbour `n` of capture target `t` is inside a region taking `t`
    /// seals off, so it stops being a threat to `t`.
    fn sealed_with(&mut self, t: usize, n: usize) -> bool {
        let Some(b) = self.board.as_ref() else { return false };
        let bound = self.ring[t].nodes;
        if bound == 0 || self.mine_now[n] {
            return false;
        }
        self.tarjan.sealed(b, &self.mine_now, t, n, bound)
    }

    /// Mean and standard deviation (both x1000) of the attackers lost taking `def`
    /// troops behind `defense` milli, from the engine's exchange loop.
    fn loss_stats(&self, def: u32, defense: u32) -> (u64, u64) {
        let d = defense.max(1) as u64;
        let per_win = (1_000_000 + d / 2) / d;
        let wins = (def as u64 * 1000).div_ceil(per_win);
        let p = self.win as u64;
        let q = 1000 - p;
        let mean = wins * q * 1000 / p;
        // Negative binomial: var = wins * q / p^2.
        let var_x1e6 = wins * q * 1_000_000_000 / (p * p);
        (mean, isqrt(var_x1e6))
    }

    /// Defenders `attackers` troops expect to kill behind `defense` milli going all in.
    fn kills(&self, attackers: u64, defense: u64) -> u64 {
        let p = self.win as u64;
        attackers * p * 1000 / ((1000 - p) * defense.max(1))
    }

    fn defense(&self, geo: &Geometry, from: usize, to: usize, wall_milli: u32) -> u64 {
        let h = self.rules.height_multiplier_milli(geo.height[from], geo.height[to]) as u64;
        (h * wall_milli as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64)
    }

    fn owned(&self, i: usize, me: PlayerId) -> bool {
        self.seen[i].is_some_and(|c| c.owner == Some(me))
    }

    fn enemy(&self, i: usize, me: PlayerId) -> bool {
        self.seen[i].is_some_and(|c| c.owner.is_some_and(|o| o != me))
    }

    /// Garrison own (or soon own) hex `h` wants against its enemy neighbours, treating
    /// `ours` as ours too; `None` when no enemy touches it.
    fn want(&self, geo: &Geometry, me: PlayerId, h: usize, ours: usize) -> Option<u64> {
        self.want_ex(geo, me, h, |n| n == ours)
    }

    /// [`Self::want`] ignoring the neighbours `skip` accepts. A hex of ours a rival would
    /// swallow a pocket through wants a bigger garrison.
    fn want_ex(&self, geo: &Geometry, me: PlayerId, h: usize, skip: impl Fn(usize) -> bool) -> Option<u64> {
        let neck = if self.k.ring && self.mine_now.get(h).copied().unwrap_or(false) {
            self.neck.get(h).copied().unwrap_or(0) as u64
        } else {
            0
        };
        self.want_base(geo, me, h, skip).map(|w| w * (100 + self.k.neck_want_pct * neck) / 100)
    }

    fn want_base(&self, geo: &Geometry, me: PlayerId, h: usize, skip: impl Fn(usize) -> bool) -> Option<u64> {
        let mut worst: Option<u64> = None;
        for &n in &geo.nbrs[h] {
            if skip(n) || !self.enemy(n, me) {
                continue;
            }
            let c = self.seen[n].unwrap();
            let stack = c.troops.saturating_sub(1) as u64;
            let wall = self.seen[h].map_or(MILLI, |c| c.wall_milli.max(MILLI));
            let mut k = self.kills(stack, self.defense(geo, n, h, wall));
            if c.owner.is_some_and(|o| self.wave.get(o as usize).copied().unwrap_or(false)) {
                k = k * (100 + self.k.wave_pct) / 100;
            }
            worst = Some(worst.map_or(k, |w| w.max(k)));
        }
        worst.map(|k| self.k.guard + k * self.k.margin_pct / 100)
    }

    /// How safe (milli) a hex holding `troops` is against `want`: concave, full at `want`.
    fn safety(troops: u64, want: Option<u64>) -> i64 {
        match want {
            None => 1000,
            Some(w) => {
                let r = (troops * 1000 / w.max(1)).min(1000) as i64;
                1000 - (1000 - r) * (1000 - r) / 1000
            }
        }
    }

    /// Hexes of the enemy pockets that would surrender if we took `to`.
    /// Passable hexes of the regions we'd close off from the board edge by taking `to`.
    fn region_gain(&self, geo: &Geometry, to: usize, me: PlayerId) -> i64 {
        let mut gain = 0i64;
        let mut done: Vec<usize> = vec![];
        for nb in geo.hexes[to].neighbors() {
            let Some(n) = geo.slot(nb) else { continue };
            if self.owned(n, me) || done.contains(&n) {
                continue;
            }
            let mut comp = vec![n];
            let mut k = 0;
            let mut enclosed = true;
            while k < comp.len() && enclosed {
                let c = comp[k];
                k += 1;
                for h in geo.hexes[c].neighbors() {
                    let Some(j) = geo.slot(h) else {
                        enclosed = false;
                        break;
                    };
                    if j != to && !self.owned(j, me) && !comp.contains(&j) {
                        comp.push(j);
                    }
                }
                if comp.len() > self.k.region_max {
                    enclosed = false;
                }
            }
            done.extend(&comp);
            if enclosed {
                gain += comp.iter().filter(|&&c| geo.passable[c]).count() as i64;
            }
        }
        gain
    }

    fn enclosure_gain(&self, geo: &Geometry, to: usize, me: PlayerId) -> i64 {
        if self.k.region {
            return self.region_gain(geo, to, me);
        }
        const LIMIT: usize = 64;
        let mut gain = 0i64;
        let mut done: Vec<usize> = vec![];
        for nb in geo.hexes[to].neighbors() {
            let Some(n) = geo.slot(nb) else { continue };
            let Some(owner) = self.seen[n].and_then(|c| c.owner) else { continue };
            if owner == me || done.contains(&n) {
                continue;
            }
            let mut comp = vec![n];
            let mut k = 0;
            let mut enclosed = true;
            while k < comp.len() && enclosed {
                let c = comp[k];
                k += 1;
                for h in geo.hexes[c].neighbors() {
                    let Some(j) = geo.slot(h).filter(|&j| geo.passable[j]) else {
                        enclosed = false;
                        break;
                    };
                    if j == to {
                        continue;
                    }
                    match self.seen[j].and_then(|c| c.owner) {
                        Some(o) if o == owner => {
                            if !comp.contains(&j) {
                                comp.push(j);
                            }
                        }
                        Some(o) if o == me => {}
                        _ => {
                            enclosed = false;
                            break;
                        }
                    }
                }
                if comp.len() > LIMIT {
                    enclosed = false;
                }
            }
            done.extend(&comp);
            if enclosed {
                gain += comp.len() as i64;
            }
        }
        gain
    }

    /// Articulation points of the land we don't own (Tarjan, iterative). Enemy hexes
    /// root the search first, so regions that hold an enemy are never a backyard.
    fn seal(&self, geo: &Geometry, me: PlayerId) -> Seal {
        let slots = geo.passable.len();
        let open = |i: usize| geo.passable[i] && !self.owned(i, me);
        let mut cuts = vec![0u32; slots];
        let mut pocket = vec![false; slots];
        let mut disc = vec![0u32; slots];
        let mut low = vec![0u32; slots];
        let mut size = vec![1u32; slots];
        let mut parent = vec![usize::MAX; slots];
        let mut timer = 0u32;
        let mut roots: Vec<usize> = (0..slots).filter(|&i| open(i) && self.enemy(i, me)).collect();
        roots.extend((0..slots).filter(|&i| open(i) && !self.enemy(i, me)));
        let mut stack: Vec<(usize, usize)> = vec![];
        let mut comp: Vec<usize> = vec![];
        for root in roots {
            if disc[root] != 0 {
                continue;
            }
            comp.clear();
            let mut has_enemy = false;
            timer += 1;
            disc[root] = timer;
            low[root] = timer;
            stack.push((root, 0));
            while let Some(top) = stack.last_mut() {
                let (v, it) = *top;
                if it < geo.nbrs[v].len() {
                    top.1 += 1;
                    let w = geo.nbrs[v][it];
                    if !open(w) {
                        continue;
                    }
                    if disc[w] == 0 {
                        parent[w] = v;
                        timer += 1;
                        disc[w] = timer;
                        low[w] = timer;
                        stack.push((w, 0));
                    } else if w != parent[v] {
                        low[v] = low[v].min(disc[w]);
                    }
                    continue;
                }
                stack.pop();
                comp.push(v);
                has_enemy |= self.enemy(v, me);
                if v == root {
                    continue;
                }
                let p = parent[v];
                low[p] = low[p].min(low[v]);
                size[p] += size[v];
                if low[v] >= disc[p] && size[v] <= self.k.seal_max && p != root {
                    cuts[p] += size[v];
                }
            }
            if !has_enemy && comp.len() as u32 <= self.k.seal_max {
                for &c in &comp {
                    pocket[c] = true;
                    cuts[c] = 0;
                }
            }
        }
        Seal { cuts, pocket }
    }

    /// The best capture. While `sprinting` nothing needs to be held past the cutoff, so
    /// every capture sends just enough to be sure and safety is ignored.
    fn best_capture(
        &self,
        geo: &Geometry,
        me: PlayerId,
        mine: &[usize],
        sprinting: bool,
        seal: Option<&Seal>,
    ) -> (Option<Capture>, Option<Capture>) {
        let mut best: Option<Capture> = None;
        // Best capture the pad can't afford yet; `send` holds the troops it is short.
        let mut blocked: Option<Capture> = None;
        // A troop is worth less the more of them we already hold per hex.
        let bank: u64 = mine.iter().map(|&i| self.seen[i].unwrap().troops as u64).sum();
        let per_hex = bank / mine.len().max(1) as u64;
        let full = self.k.cost_pct as u64 * 10;
        let cost_milli = match self.k.wealth {
            0 => full,
            w => full * w / (w + per_hex),
        } as i64;
        for &s in mine {
            let src = self.seen[s].unwrap();
            if src.troops < 2 {
                continue;
            }
            let troops = src.troops as u64;
            let avail = troops - 1;
            let want_s_before = self.want(geo, me, s, usize::MAX);
            let before = Self::safety(troops, want_s_before);
            for &t in &geo.nbrs[s] {
                let Some(tc) = self.seen[t] else { continue };
                if tc.owner == Some(me) {
                    continue;
                }
                let defense = self.defense(geo, s, t, tc.wall_milli) as u32;
                let (mean, sd) = self.loss_stats(tc.troops, defense);
                let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
                let loss = mean.div_ceil(1000);
                let want_t = if self.k.ring && self.ring[t].hexes > 0 {
                    let mask = self.sealed_mask[t];
                    self.want_ex(geo, me, t, |n| {
                        n == s || geo.nbrs[t].iter().position(|&x| x == n).is_some_and(|k| mask & (1 << k) != 0)
                    })
                } else {
                    self.want(geo, me, t, s)
                };
                if need > avail {
                    if self.k.stage > 0 {
                        let full = need.max(want_t.map_or(0, |w| loss + w));
                        let short = full + 1 - troops.min(full + 1);
                        let score = self.capture_gain(geo, me, t, tc, seal)
                            - loss as i64 * cost_milli / 1000
                            - short as i64 * self.k.short_cost;
                        if blocked.map_or(true, |b| (score, std::cmp::Reverse(t)) > (b.score, std::cmp::Reverse(b.to))) {
                            blocked = Some(Capture { score, from: s, to: t, send: short as u32 });
                        }
                    }
                    continue;
                }
                let want_s = self.want(geo, me, s, t);
                let mut cands = [0u64; 12];
                for (f, c) in cands.iter_mut().enumerate().take(9) {
                    *c = need + (avail - need) * f as u64 / 8;
                }
                cands[9] = want_t.map_or(avail, |w| loss + w);
                cands[10] = want_s.map_or(avail, |w| troops.saturating_sub(w));
                cands[11] = avail;
                let mut pick = (i64::MIN, 0i64, avail);
                for c in cands {
                    let send = c.clamp(need, avail);
                    let st = Self::safety(send.saturating_sub(loss), want_t);
                    let drop = before - Self::safety(troops - send, want_s);
                    let v = st * 1000 - drop * self.k.drop_w * 10;
                    let better = if self.k.lean {
                        (v, std::cmp::Reverse(send)) > (pick.0, std::cmp::Reverse(pick.2))
                    } else {
                        (v, send) > (pick.0, pick.2)
                    };
                    if better {
                        pick = (v, st, send);
                    }
                }
                if sprinting {
                    pick = (1_000_000, 1000, need.max(avail * self.k.send_pct / 100).min(avail));
                }
                let (v, st, send) = pick;
                if st < self.k.safe_min {
                    continue;
                }
                let gain = self.capture_gain(geo, me, t, tc, seal);
                let score = gain * st / 1000 + (v / 1000 - st) * self.k.land / 1000
                    - loss as i64 * cost_milli / 1000;
                let better = best.as_ref().map_or(true, |b| {
                    (score, std::cmp::Reverse(t), send)
                        > (b.score, std::cmp::Reverse(b.to), b.send as u64)
                });
                if better {
                    best = Some(Capture { score, from: s, to: t, send: send as u32 });
                }
            }
        }
        (best, blocked)
    }

    /// Score of holding `t` once taken, before troop costs.
    fn capture_gain(&self, geo: &Geometry, me: PlayerId, t: usize, tc: Cell, seal: Option<&Seal>) -> i64 {
        let hugged = geo.nbrs[t].iter().filter(|&&n| self.owned(n, me)).count() as i64;
        let exposed = geo.nbrs[t].len() as i64 - hugged;
        let mut gain = self.k.land;
        if self.k.ring {
            gain += self.ring_value(&self.ring[t]) + self.setup[t] + self.k.neck_take * self.neck[t] as i64;
        } else if self.k.region {
            gain += self.k.enclose * self.enclosure_gain(geo, t, me);
        }
        if let Some(o) = tc.owner {
            if !self.k.region && !self.k.ring {
                gain += self.k.enclose * self.enclosure_gain(geo, t, me);
            }
            gain += self.k.enemy;
            gain += match tc.building {
                Some(Building::Village) => self.k.loot_village,
                Some(Building::Farm | Building::Quarry) => self.k.loot_farm,
                _ => 0,
            };
            if self.k.focus > 0 {
                let n = self.land_of.get(o as usize).copied().unwrap_or(0).max(1) as i64;
                gain += self.k.focus * 50 / n;
            }
            if self.prey_of == Some(o) {
                gain += self.k.prey + if tc.building == Some(Building::Village) { self.k.prey_village } else { 0 };
            }
        }
        gain += self.k.hug * hugged - self.k.open * exposed + self.k.height * geo.height[t] as i64;
        if let Some(seal) = seal {
            gain += self.k.seal * seal.cuts[t] as i64;
            if seal.pocket[t] {
                gain -= self.k.backyard;
            }
        }
        gain
    }

    /// Walks the nearest spare stack one step toward `pad` (or gathers onto it while
    /// nobody touches our land early on).
    fn stage_to(&self, geo: &Geometry, me: PlayerId, pad: usize, calm: bool) -> Option<Action> {
        if calm {
            return Some(Action::Gather { target: geo.hexes[pad] });
        }
        let mut prev = vec![usize::MAX; geo.passable.len()];
        prev[pad] = pad;
        let mut queue = VecDeque::from([pad]);
        while let Some(c) = queue.pop_front() {
            if c != pad && self.spare(geo, me, c) >= 2 {
                let troops = self.spare(geo, me, c) as u32;
                return Some(Action::Move { from: geo.hexes[c], to: geo.hexes[prev[c]], troops });
            }
            for &n in &geo.nbrs[c] {
                if prev[n] == usize::MAX && self.owned(n, me) {
                    prev[n] = c;
                    queue.push_back(n);
                }
            }
        }
        None
    }

    /// Spare troops on own hex `i` beyond what it wants to keep.
    fn spare(&self, geo: &Geometry, me: PlayerId, i: usize) -> u64 {
        let t = self.seen[i].unwrap().troops as u64;
        let keep = self.want(geo, me, i, usize::MAX).unwrap_or(0).max(1);
        t.saturating_sub(keep)
    }

    /// Moves a neighbour's spare troops into the freshest-taken hex still short of its
    /// garrison, so it isn't simply retaken. Returns the safety gain (milli).
    fn reinforce(&self, geo: &Geometry, me: PlayerId, mine: &[usize], tick: u32) -> Option<(i64, Action)> {
        let mut best: Option<(i64, usize, usize, u64)> = None;
        for &f in mine {
            let at = self.flipped.get(f).copied().unwrap_or(0);
            if at == 0 || tick >= at + self.k.reinforce {
                continue;
            }
            let Some(w) = self.want(geo, me, f, usize::MAX) else { continue };
            let have = self.seen[f].unwrap().troops as u64;
            if have >= w {
                continue;
            }
            for &n in &geo.nbrs[f] {
                if !self.owned(n, me) {
                    continue;
                }
                let spare = self.spare(geo, me, n);
                if spare == 0 {
                    continue;
                }
                let gain = Self::safety(have + spare, Some(w)) - Self::safety(have, Some(w));
                if best.is_none_or(|b| (gain, std::cmp::Reverse(n)) > (b.0, std::cmp::Reverse(b.1))) {
                    best = Some((gain, n, f, spare));
                }
            }
        }
        let (gain, n, f, spare) = best?;
        Some((gain, Action::Move { from: geo.hexes[n], to: geo.hexes[f], troops: spare as u32 }))
    }

    /// Relays the spare stack that buys the most border safety per step one hex toward
    /// the short border hex nearest to it. Returns the safety gain per step (milli).
    fn supply(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<(i64, Action)> {
        let slots = geo.passable.len();
        // Multi-source BFS from every short border hex through our land.
        let mut dist = vec![u32::MAX; slots];
        let mut origin = vec![usize::MAX; slots];
        let mut toward = vec![usize::MAX; slots];
        let mut short: Vec<(u64, u64)> = vec![(0, 0); slots];
        let mut queue = VecDeque::new();
        for &f in mine {
            let Some(w) = self.want(geo, me, f, usize::MAX) else { continue };
            let have = self.seen[f].unwrap().troops as u64;
            if have >= w {
                continue;
            }
            short[f] = (have, w);
            dist[f] = 0;
            origin[f] = f;
            queue.push_back(f);
        }
        while let Some(c) = queue.pop_front() {
            for &n in &geo.nbrs[c] {
                if dist[n] == u32::MAX && self.owned(n, me) {
                    dist[n] = dist[c] + 1;
                    origin[n] = origin[c];
                    toward[n] = c;
                    queue.push_back(n);
                }
            }
        }
        let mut best: Option<(i64, usize)> = None;
        for &i in mine {
            if dist[i] == 0 || dist[i] == u32::MAX {
                continue;
            }
            let spare = self.spare(geo, me, i);
            if spare == 0 {
                continue;
            }
            let (have, w) = short[origin[i]];
            let gain = (Self::safety(have + spare, Some(w)) - Self::safety(have, Some(w)))
                / dist[i] as i64;
            if best.map_or(true, |b| (gain, std::cmp::Reverse(i)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((gain, i));
            }
        }
        let (gain, i) = best?;
        let troops = self.spare(geo, me, i) as u32;
        Some((gain, Action::Move { from: geo.hexes[i], to: geo.hexes[toward[i]], troops }))
    }

    /// Nothing worth taking: pull troops toward the border hex facing the cheapest target.
    fn regroup(&self, geo: &Geometry, me: PlayerId, mine: &[usize], tick: u32) -> Option<Action> {
        if mine.len() < 2 {
            return None;
        }
        let mut cheapest: Option<(u32, usize)> = None;
        let mut contact = false;
        for &s in mine {
            for &t in &geo.nbrs[s] {
                let Some(tc) = self.seen[t].filter(|c| c.owner != Some(me)) else { continue };
                contact |= tc.owner.is_some();
                let key = (tc.troops, s);
                if cheapest.map_or(true, |c| key < c) {
                    cheapest = Some(key);
                }
            }
        }
        let (_, pad) = cheapest?;
        // Once enemies touch us a Gather would strip every far border hex down to one
        // troop, so stage single stacks instead.
        let late_gather = self.k.late_gather && self.k.late_at.is_some();
        self.stage_to(geo, me, pad, late_gather || (!contact && tick < self.k.gather_until))
    }
}

fn isqrt(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as u64;
    while x * x > v {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= v {
        x += 1;
    }
    x
}

impl Bot for Core {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if let Some(r) = &view.rules {
            self.rules = r.clone();
        }
        self.win = self.rules.attacker_win_milli(view.tick);
        let alive = view.public_players.iter().filter(|p| p.alive).count();
        let lobby = view.public_players.len();
        if self.duel.is_some() && (lobby == 2 || alive <= self.ffa_duel_when || self.dominant) {
            self.k = self.duel.take().unwrap();
            self.end = None;
        }
        if self.end.is_some() && lobby > 2 && alive <= self.ffa_end_when {
            self.k = self.end.take().unwrap();
        }
        if self.k.open_until > 0 {
            self.k.opening(view.tick);
        }
        if self.k.late_from > 0 && view.tick >= self.k.late_from {
            self.k.go_late();
        }
        if self.k.duel_blitz && view.public_players.iter().filter(|p| p.alive && p.id != view.you).count() == 1 {
            self.k.duel_blitz = false;
            self.calm_late = None;
            self.k.late_sprint = true;
            self.k.late_gather = true;
            self.k.send_pct = self.k.blitz_send_pct;
            self.k.push_gather = self.k.blitz_push;
        }
        let geo = self.geo.take().unwrap_or_else(|| Geometry::new(view));
        if self.seen.is_empty() {
            self.seen = vec![None; geo.passable.len()];
        }
        let me = view.you;
        let mut mine = vec![];
        for c in &view.visible_hexes {
            let Some(i) = geo.slot(c.hex) else { continue };
            self.seen[i] = Some(Cell {
                owner: c.owner,
                troops: c.troops,
                wall_milli: c.wall_milli,
                building: c.building.as_ref().map(|b| b.kind),
            });
            if c.owner == Some(me) {
                mine.push(i);
            }
        }
        // A hex we remember owning but can't see any more was lost (a pocket swallowed
        // beyond fog range); forget the owner rather than count it as ours.
        let slots = geo.passable.len();
        if self.k.reinforce > 0 {
            self.flipped.resize(slots, 0);
            if !self.mine_now.is_empty() {
                for &i in &mine {
                    if !self.mine_now[i] {
                        self.flipped[i] = view.tick;
                    }
                }
            }
        }
        self.mine_now.clear();
        self.mine_now.resize(slots, false);
        for &i in &mine {
            self.mine_now[i] = true;
        }
        if self.duel.is_some() && self.ffa_duel_share > 0 {
            let land = geo.passable.iter().filter(|&&p| p).count();
            self.dominant = mine.len() * 100 >= land * self.ffa_duel_share;
        }
        for i in 0..slots {
            if let Some(c) = self.seen[i].as_mut() {
                if c.owner == Some(me) && !self.mine_now[i] {
                    c.owner = None;
                }
            }
        }
        let finish_on = self.k.finish_alive > 0 || self.k.finish_land_pct > 0;
        let duel = view.public_players.iter().filter(|p| p.alive && p.id != me).count() == 1;
        let duel_early = duel && self.k.late_from > 0 && self.k.duel_late_from > 0 && view.tick >= self.k.duel_late_from;
        let wave_on = lobby > 2 && self.k.wave_pct > 0 && (self.k.wave_from..self.k.wave_until).contains(&view.tick);
        if self.k.focus > 0 || finish_on || duel_early || wave_on {
            self.land_of = vec![0; 16];
            for o in self.seen.iter().flatten().filter_map(|c| c.owner) {
                if let Some(n) = self.land_of.get_mut(o as usize) {
                    *n += 1;
                }
            }
        }
        self.wave = [false; 16];
        if wave_on {
            for (o, w) in self.wave.iter_mut().enumerate() {
                *w = o != me as usize && self.land_of.get(o).copied().unwrap_or(0) as usize >= mine.len();
            }
        }
        if duel_early {
            let rival = view
                .public_players
                .iter()
                .filter(|p| p.alive && p.id != me)
                .map(|p| self.land_of.get(p.id as usize).copied().unwrap_or(0) as usize)
                .max()
                .unwrap_or(0);
            if mine.len() * 100 >= rival * self.k.duel_lead_pct as usize {
                self.k.go_late();
            }
        }
        if finish_on {
            let alive = view.public_players.iter().filter(|p| p.alive).count();
            let passable = geo.passable.iter().filter(|&&p| p).count().max(1);
            let biggest = view
                .public_players
                .iter()
                .filter(|p| p.alive && p.id != me)
                .map(|p| self.land_of.get(p.id as usize).copied().unwrap_or(0) as usize)
                .max()
                .unwrap_or(0);
            let leading = mine.len() * 100 >= biggest * self.k.finish_lead_pct as usize;
            let finishing = leading
                && ((self.k.finish_alive > 0 && alive <= self.k.finish_alive)
                    || (self.k.finish_land_pct > 0 && mine.len() * 100 >= passable * self.k.finish_land_pct as usize));
            let changed = finishing != self.finishing;
            self.finishing = finishing;
            if changed && self.k.finish_blitz > 0 {
                if finishing {
                    self.calm_late = Some((self.k.late_sprint, self.k.late_gather, self.k.send_pct, self.k.push_gather));
                    if self.k.finish_blitz == 1 {
                        self.k.late_sprint = true;
                        self.k.send_pct = self.k.blitz_send_pct;
                    }
                    self.k.late_gather = true;
                    self.k.push_gather = self.k.blitz_push;
                } else if let Some((sprint, gather, send, push)) = self.calm_late.take() {
                    self.k.late_sprint = sprint;
                    self.k.late_gather = gather;
                    self.k.send_pct = send;
                    self.k.push_gather = push;
                }
            }
        }
        self.prey_of = None;
        if self.finishing && self.k.prey > 0 {
            let mut touching = [false; 16];
            if self.k.prey_border {
                for &i in &mine {
                    for &n in &geo.nbrs[i] {
                        if let Some(o) = self.seen[n].and_then(|c| c.owner).filter(|&o| o != me) {
                            if let Some(t) = touching.get_mut(o as usize) {
                                *t = true;
                            }
                        }
                    }
                }
            }
            self.prey_of = view
                .public_players
                .iter()
                .filter(|p| p.alive && p.id != me)
                .filter(|p| !self.k.prey_border || touching.get(p.id as usize).copied().unwrap_or(false))
                .filter_map(|p| self.land_of.get(p.id as usize).filter(|&&n| n > 0).map(|&n| (n, p.id)))
                .min()
                .map(|(_, id)| id);
        }
        if self.k.ring {
            if self.board.is_none() {
                self.board = Some(Board::new(view));
            }
            self.analyse(&geo, me);
            self.sealed_mask.clear();
            self.sealed_mask.resize(slots, 0);
            for t in 0..slots {
                if self.ring[t].hexes == 0 || !geo.nbrs[t].iter().any(|&n| self.mine_now[n]) {
                    continue;
                }
                let mut mask = 0u8;
                for k in 0..geo.nbrs[t].len() {
                    let n = geo.nbrs[t][k];
                    if self.enemy(n, me) && self.sealed_with(t, n) {
                        mask |= 1 << k;
                    }
                }
                self.sealed_mask[t] = mask;
            }
        }
        let sprinting = view.tick + self.k.sprint >= self.rules.max_ticks
            || (self.k.late_sprint && self.k.late_at.is_some());
        let seal = (self.k.seal > 0 || self.k.backyard > 0).then(|| self.seal(&geo, me));
        let supply = self.supply(&geo, me, &mine);
        let (capture, blocked) = self.best_capture(&geo, me, &mine, sprinting, seal.as_ref());
        let capture = capture.filter(|c| sprinting || c.score >= self.k.min_score);
        let ring_of = |c: &Capture| if self.k.ring { self.ring_value(&self.ring[c.to]) } else { 0 };
        let ring_now = capture.as_ref().map_or(0, ring_of);
        self.chosen = None;
        if self.k.ring && self.k.ring_first > 0 && ring_now >= self.k.ring_first {
            let c = capture.unwrap();
            let action = Action::Move { from: geo.hexes[c.from], to: geo.hexes[c.to], troops: c.send };
            self.chosen = Some((geo.hexes[c.to], ring_now));
            self.geo = Some(geo);
            return PlayerCommand::action(action);
        }
        if let Some(a) = self.ring_stage(&geo, me, &mine, ring_now) {
            self.geo = Some(geo);
            return PlayerCommand::action(a);
        }
        let every = match self.k.wave_supply_every {
            e if wave_on && e > 0 => e,
            _ => self.k.supply_every,
        };
        let supply_tick = !sprinting && every > 0 && view.tick % every == 0;
        let floor = capture.map_or(self.k.min_score, |c| c.score);
        let staging = blocked
            .filter(|b| !sprinting && mine.len() > 1 && b.score >= floor + self.k.stage)
            .and_then(|b| {
                let contact =
                    mine.iter().any(|&s| geo.nbrs[s].iter().any(|&n| self.enemy(n, me)));
                self.stage_to(&geo, me, b.from, !contact && view.tick < self.k.gather_until)
            });
        let push = self.k.late_at.is_some()
            && self.k.push_gather > 0
            && view.tick % self.k.push_gather == 0;
        let pad = capture.or(blocked).map(|c| c.from);
        let reinforce = (!sprinting && self.k.reinforce > 0)
            .then(|| self.reinforce(&geo, me, &mine, view.tick))
            .flatten()
            .filter(|r| r.0 >= self.k.reinforce_min);
        let action = match (supply, capture) {
            _ if push && pad.is_some() => Action::Gather { target: geo.hexes[pad.unwrap()] },
            (Some((gain, a)), _) if supply_tick && gain >= self.k.supply_min => a,
            _ if reinforce.is_some() => reinforce.unwrap().1,
            _ if staging.is_some() => staging.unwrap(),
            (_, Some(c)) => {
                Action::Move { from: geo.hexes[c.from], to: geo.hexes[c.to], troops: c.send }
            }
            (Some((gain, a)), None) if gain > 0 => a,
            _ => self.regroup(&geo, me, &mine, view.tick).unwrap_or(Action::NoOp),
        };
        if let (Some(c), Action::Move { to, .. }) = (capture, &action) {
            if *to == geo.hexes[c.to] && ring_now > 0 {
                self.chosen = Some((*to, ring_now));
            }
        }
        self.geo = Some(geo);
        PlayerCommand::action(action)
    }
}
