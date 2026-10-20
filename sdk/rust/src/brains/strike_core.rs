//! StrikeCore: Ridgeline's border play, as frozen in Pangolin v9 (Claude-Ridgeline and
//! Claude-Pangolin, reused with credit), plus the attack terms Striker needs: a value for
//! the income a capture takes from its owner and gives us (enemy Villages, Farms and
//! Quarries are not bare land), a bonus for the hexes of one chosen victim, a supply
//! weighting toward the front facing it, and a finishing mode that spends every action
//! on the victim's last hexes.
//!
//! Ridgeline's design notes:
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
    /// On-board neighbour slots of each slot, impassable ones included: a v0.7 region
    /// flows through mountains and water, and only the board edge stops it closing.
    ring: Vec<Vec<usize>>,
    on_board: Vec<bool>,
    /// On-board slots with an off-board neighbour.
    edge: Vec<bool>,
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
        let mut g = Geometry {
            radius,
            width,
            passable,
            height,
            nbrs: vec![],
            hexes,
            ring: vec![],
            on_board: vec![false; slots],
            edge: vec![false; slots],
        };
        for i in 0..slots {
            g.on_board[i] = g.slot(g.hexes[i]) == Some(i);
        }
        g.ring = (0..slots)
            .map(|i| {
                if !g.on_board[i] {
                    return vec![];
                }
                g.hexes[i].neighbors().iter().filter_map(|n| g.slot(*n)).collect()
            })
            .collect();
        for i in 0..slots {
            g.edge[i] = g.on_board[i] && g.ring[i].len() < 6;
        }
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
    /// Troops a tick the hex made for its owner when last seen.
    income: u32,
}

/// One affordable capture under consideration.
#[derive(Clone, Copy)]
struct Capture {
    score: i64,
    from: usize,
    to: usize,
    send: u32,
    /// Passable hexes the capture would seal off.
    sealed: u32,
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

/// What each capture would seal off from the board edge this tick (rules v0.7 region
/// capture): taking slot `t` turns every region of not-ours hexes it cuts off into ours.
/// The scratch buffers are kept between ticks so the pass allocates nothing.
#[derive(Default)]
struct Regions {
    /// Passable hexes taking `t` would seal off.
    land: Vec<u32>,
    /// Of those, hexes we last saw a rival own.
    enemy: Vec<u32>,
    /// Of those, hexes we own (nonzero only when the capturer is a rival).
    mine: Vec<u32>,
    /// Troops last seen on the sealed hexes we don't own: they change sides with them.
    troops: Vec<u32>,
    /// Villages a rival owns among the sealed hexes: buildings change sides with them.
    villages: Vec<u32>,
    /// Buildings of ours among the sealed hexes (for threats against us).
    mine_bld: Vec<u32>,
    disc: Vec<u32>,
    low: Vec<u32>,
    sub_land: Vec<u32>,
    sub_enemy: Vec<u32>,
    sub_mine: Vec<u32>,
    sub_troops: Vec<u32>,
    sub_villages: Vec<u32>,
    sub_mine_bld: Vec<u32>,
    parent: Vec<usize>,
    stack: Vec<(usize, usize)>,
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
    /// From `late_from` on, Gather toward the cheapest target when no capture is ready (0/1).
    late_gather: bool,
    /// Tick the late phase began.
    late_at: Option<u32>,
    /// Bonus for an enemy hex of a rival we know `n` hexes of: `focus * 50 / n`, so the
    /// last hexes of a dying rival come first.
    focus: i64,
    /// Score per troop a tick of income a capture moves from its owner to us (counting
    /// both the owner's loss and our gain), so Villages and Farms outrank bare land.
    bld: i64,
    /// Score for any hex of the victim.
    victim: i64,
    /// % of a destroyed enemy garrison counted in a capture's score, at the troop price
    /// a capture's own losses are charged.
    kill_pct: i64,
    /// % weight of supply toward short border hexes that face the victim.
    vsupply_pct: i64,
    /// Supply cadence from the late phase on.
    late_supply_every: u32,
    /// While finishing, score for a victim hex on top of `victim`.
    finish_bonus: i64,
    /// A victim is picked from this tick on (0 = never).
    victim_from: u32,
    /// Finish the victim once we know at most this many of its hexes (0 = never).
    finish_n: u32,
    /// A new victim must have at most this % of the current one's known land to replace it.
    switch_pct: u32,
    /// Score per passable hex a capture seals off from the board edge (0 = ignore regions).
    region: i64,
    /// Extra score per sealed-off hex a rival owns.
    region_enemy: i64,
    /// Walk spare stacks toward a capture we can't afford yet when it would seal off at
    /// least this many hexes (0 = never).
    region_stage: u32,
    /// Score lost for taking a hex that changed hands in the last `churn_ticks` ticks:
    /// those are the border hexes that trade back and forth without moving the front.
    churn_pen: i64,
    churn_ticks: u32,
    /// The churn penalty applies from this tick (before it, fresh enemy expansion is thin
    /// land worth taking).
    churn_from: u32,
    /// Score per hex of ours a rival would swallow by taking a hex we can take first.
    neck_take: i64,
    /// Extra % of garrison per hex of ours a rival would swallow by taking this one.
    neck_want_pct: u64,
    /// Most swallowed hexes the neck terms count.
    neck_cap: u32,
    /// Hexes each building of ours in a pocket counts as in the neck terms.
    neck_bld: u32,
    /// Hexes we took in the last this many ticks count as fresh (0 = never reinforce).
    reinforce: u32,
    /// Safety gain (milli) a move of spare troops into a fresh hex must buy to take the tick.
    reinforce_min: i64,
    /// Demolish a Village, Farm or Quarry on a hex we took once the biggest enemy stack
    /// next to it would kill this % of its garrison (0 = never): it falls back bare.
    demolish_pct: u64,
    /// Board hexes the neck passes may visit a tick, one region pass per rival costing a
    /// board's worth (0 = every rival every tick).
    neck_budget: usize,
    /// Finish the victim once at most this many players are alive, in lobbies that
    /// started with more (0 = never).
    blitz_alive: u32,
    /// Finish the victim once we hold this % of the board's passable hexes (0 = never).
    blitz_share_pct: u32,
    /// In 1v1 lobbies, the late phase starts at this tick instead (0 = same as 8-player).
    duel_late_from: u32,
    /// In 1v1 lobbies, play the late sprint (0/1).
    duel_late_sprint: bool,
    /// % of the troops a seal hands over counted in its score, at the troop price.
    region_troops_pct: i64,
    /// Extra score per rival Village a seal takes.
    region_village: i64,
    /// While finishing, skip supply, staging and reinforcement everywhere (0/1).
    finish_rush: bool,
    /// Score for a capture with at least 4 of our hexes around it: it fills a notch.
    notch: i64,
    /// Score lost for a capture facing an enemy that leaves the new hex with at most 2
    /// of ours around it and short of its garrison: a tendril a rival pinches off.
    thin_pen: i64,
    /// Extra % of garrison an own hex facing an enemy wants when at most 2 of ours are
    /// around it.
    thin_want_pct: u64,
    /// From this tick captures ignore the safety bar (0 = never): the game ends on hex
    /// count, so a hex held at the cutoff is worth the risk.
    cash_from: u32,
    /// Hold the late switch while we own less than this many thousandths of the board's
    /// passable hexes (0 = switch regardless): a small seat that goes reckless is eaten.
    late_share_milli: usize,
    /// Whether the duel overrides have been applied.
    duel_set: bool,
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
            late_gather: k("late_gather", 0) > 0,
            late_at: None,
            focus: k("focus", 0),
            bld: k("bld", 0),
            victim: k("victim", 0),
            kill_pct: k("kill_pct", 0),
            vsupply_pct: k("vsupply_pct", 100),
            late_supply_every: k("late_supply_every", 3) as u32,
            finish_bonus: k("finish_bonus", 0),
            victim_from: k("victim_from", 0) as u32,
            finish_n: k("finish_n", 0) as u32,
            switch_pct: k("switch_pct", 70) as u32,
            region: k("region", 0),
            region_enemy: k("region_enemy", 0),
            region_stage: k("region_stage", 0) as u32,
            churn_pen: k("churn_pen", 0),
            churn_ticks: k("churn_ticks", 20) as u32,
            churn_from: k("churn_from", 3000) as u32,
            neck_take: k("neck_take", 0),
            neck_want_pct: k("neck_want_pct", 0) as u64,
            neck_cap: k("neck_cap", 30) as u32,
            neck_bld: k("neck_bld", 0) as u32,
            reinforce: k("reinforce", 0) as u32,
            reinforce_min: k("reinforce_min", 300),
            demolish_pct: k("demolish_pct", 0) as u64,
            neck_budget: k("neck_budget", 0) as usize,
            blitz_alive: k("blitz_alive", 0) as u32,
            blitz_share_pct: k("blitz_share_pct", 0) as u32,
            duel_late_from: k("duel_late_from", 0) as u32,
            duel_late_sprint: k("duel_late_sprint", 0) > 0,
            region_troops_pct: k("region_troops_pct", 0),
            region_village: k("region_village", 0),
            finish_rush: k("finish_rush", 1) > 0,
            notch: k("notch", 0),
            thin_pen: k("thin_pen", 0),
            thin_want_pct: k("thin_want_pct", 0) as u64,
            cash_from: k("cash_from", 0) as u32,
            late_share_milli: k("late_share_milli", 0) as usize,
            duel_set: false,
        }
    }

    fn go_late(&mut self) {
        self.margin_pct = self.late_margin_pct;
        self.safe_min = self.late_safe_min;
        self.min_score = self.late_min_score;
        self.sure_x10 = self.late_sure_x10;
        self.enemy = self.late_enemy;
        self.supply_every = self.late_supply_every;
        self.late_at = Some(self.late_from);
        self.late_from = 0;
    }
}

pub struct StrikeCore {
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
    /// The rival every attack term favours, picked by the brain wrapping this core.
    pub victim: Option<PlayerId>,
    /// Spend every action on the victim's hexes: captures only need to be sure.
    pub finishing: bool,
    /// Last tick the victim touched our land.
    victim_seen: u32,
    /// Scratch space for the region pass.
    region_buf: Regions,
    /// Scratch space for the per-rival neck passes.
    neck_buf: Regions,
    /// Most of our hexes one rival capture of each slot would swallow, this tick.
    neck: Vec<u32>,
    /// Each rival's neck map as of its last pass.
    neck_of: Vec<Vec<u32>>,
    /// The rival whose neck pass ran last.
    neck_turn: usize,
    /// Tick each slot was last seen changing owner.
    flipped: Vec<u32>,
    tick: u32,
    /// Our hexes as of the last tick, and the board's passable hexes.
    my_land: usize,
    board_land: usize,
}

impl StrikeCore {
    pub fn new(style: Style) -> Self {
        StrikeCore {
            name: style.id.clone(),
            rules: RulesConfig::default(),
            win: RulesConfig::default().attacker_exchange_win_milli,
            k: Knobs::read(&style),
            geo: None,
            seen: vec![],
            land_of: vec![],
            my_land: 0,
            board_land: 0,
            victim: None,
            finishing: false,
            victim_seen: 0,
            region_buf: Regions::default(),
            neck_buf: Regions::default(),
            neck: vec![],
            neck_of: vec![],
            neck_turn: 0,
            flipped: vec![],
            tick: 0,
        }
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
        let mut worst: Option<u64> = None;
        for &n in &geo.nbrs[h] {
            if n == ours || !self.enemy(n, me) {
                continue;
            }
            let stack = self.seen[n].unwrap().troops.saturating_sub(1) as u64;
            let k = self.kills(stack, self.defense(geo, n, h, MILLI));
            worst = Some(worst.map_or(k, |w| w.max(k)));
        }
        let worst = worst?;
        let owned = self.owned(h, me);
        let neck = if owned { self.neck.get(h).copied().unwrap_or(0) as u64 } else { 0 };
        let thin = owned && self.k.thin_want_pct > 0 && geo.nbrs[h].iter().filter(|&&n| self.owned(n, me)).count() <= 2;
        let pct = 100 + self.k.neck_want_pct * neck + if thin { self.k.thin_want_pct } else { 0 };
        Some((self.k.guard + worst * self.k.margin_pct / 100) * pct / 100)
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
    fn enclosure_gain(&self, geo: &Geometry, to: usize, me: PlayerId) -> i64 {
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

    /// Region cuts (Tarjan, iterative) over every on-board hex we don't own, impassable
    /// ones included, rooted at a virtual outside node joined to the board edge. Taking a
    /// passable hex `v` seals off each DFS child subtree whose low link can't climb above
    /// `v`. Ownership of the not-ours side doesn't matter for the cut, only for its value.
    /// `by` is the capturer: us, or a rival when looking for the necks of our pockets.
    fn regions(&self, geo: &Geometry, me: PlayerId, by: PlayerId, r: &mut Regions) {
        let slots = geo.passable.len();
        let owner = |i: usize| self.seen[i].and_then(|c| c.owner);
        let open = |i: usize| geo.on_board[i] && owner(i) != Some(by);
        for v in [
            &mut r.land,
            &mut r.enemy,
            &mut r.mine,
            &mut r.disc,
            &mut r.low,
            &mut r.sub_land,
            &mut r.sub_enemy,
            &mut r.sub_mine,
            &mut r.troops,
            &mut r.sub_troops,
            &mut r.villages,
            &mut r.sub_villages,
            &mut r.mine_bld,
            &mut r.sub_mine_bld,
        ] {
            v.clear();
            v.resize(slots, 0);
        }
        r.parent.clear();
        r.parent.resize(slots, usize::MAX);
        r.stack.clear();
        let Regions {
            land,
            enemy,
            mine,
            troops,
            villages,
            disc,
            low,
            sub_land,
            sub_enemy,
            sub_mine,
            sub_troops,
            sub_villages,
            mine_bld,
            sub_mine_bld,
            parent,
            stack,
        } = r;
        // The outside node has discovery time 1; every edge hex hangs off it.
        let mut timer = 1u32;
        for root in 0..slots {
            if !geo.edge[root] || !open(root) || disc[root] != 0 {
                continue;
            }
            timer += 1;
            disc[root] = timer;
            low[root] = 1;
            stack.push((root, 0));
            while let Some(top) = stack.last_mut() {
                let (v, it) = *top;
                if it < geo.ring[v].len() {
                    top.1 += 1;
                    let w = geo.ring[v][it];
                    if !open(w) {
                        continue;
                    }
                    if disc[w] == 0 {
                        parent[w] = v;
                        timer += 1;
                        disc[w] = timer;
                        low[w] = if geo.edge[w] { 1 } else { timer };
                        stack.push((w, 0));
                    } else if w != parent[v] {
                        low[v] = low[v].min(disc[w]);
                    }
                    continue;
                }
                stack.pop();
                if geo.passable[v] {
                    sub_land[v] += 1;
                    match owner(v) {
                        Some(o) if o == me => sub_mine[v] += 1,
                        Some(_) => sub_enemy[v] += 1,
                        None => {}
                    }
                    if owner(v) == Some(me) && self.seen[v].is_some_and(|c| c.building.is_some()) {
                        sub_mine_bld[v] += 1;
                    }
                    if owner(v) != Some(me) {
                        sub_troops[v] += self.seen[v].map_or(0, |c| c.troops);
                        if owner(v).is_some() && self.seen[v].is_some_and(|c| c.building == Some(Building::Village)) {
                            sub_villages[v] += 1;
                        }
                    }
                }
                let p = parent[v];
                if p == usize::MAX {
                    continue;
                }
                low[p] = low[p].min(low[v]);
                sub_land[p] += sub_land[v];
                sub_enemy[p] += sub_enemy[v];
                sub_mine[p] += sub_mine[v];
                sub_troops[p] += sub_troops[v];
                sub_villages[p] += sub_villages[v];
                sub_mine_bld[p] += sub_mine_bld[v];
                if low[v] >= disc[p] {
                    land[p] += sub_land[v];
                    enemy[p] += sub_enemy[v];
                    mine[p] += sub_mine[v];
                    troops[p] += sub_troops[v];
                    villages[p] += sub_villages[v];
                    mine_bld[p] += sub_mine_bld[v];
                }
            }
        }
    }

    /// One region pass per rival we have seen (Mason's neck defence, reused with credit):
    /// `neck[i]` is the most of our hexes a rival next to slot `i` would swallow by taking it.
    fn necks(&mut self, geo: &Geometry, me: PlayerId) {
        let slots = geo.passable.len();
        self.neck.clear();
        self.neck.resize(slots, 0);
        let mut rivals = [false; 64];
        for o in self.seen.iter().flatten().filter_map(|c| c.owner) {
            if o != me && (o as usize) < rivals.len() {
                rivals[o as usize] = true;
            }
        }
        if self.neck_of.len() < rivals.len() {
            self.neck_of.resize(rivals.len(), vec![]);
        }
        // Passes fitting the budget run for the next rivals in turn and the others keep
        // their maps from their last turn, so a big board costs a pass or two a tick
        // while a procedural one still gets every rival's every tick.
        let passes = match self.k.neck_budget {
            0 => rivals.len(),
            b => (b / geo.on_board.iter().filter(|&&o| o).count().max(1)).max(1),
        };
        let n = rivals.len();
        let due: Vec<usize> =
            (1..=n).map(|d| (self.neck_turn + d) % n).filter(|&r| rivals[r]).take(passes).collect();
        let mut buf = std::mem::take(&mut self.neck_buf);
        for &r in &due {
            self.neck_turn = r;
            let rid = r as PlayerId;
            self.regions(geo, me, rid, &mut buf);
            let mut map = std::mem::take(&mut self.neck_of[r]);
            map.clear();
            map.extend((0..slots).map(|i| {
                let m = buf.mine[i] + buf.mine_bld[i] * self.k.neck_bld;
                let next_to = m > 0 && geo.nbrs[i].iter().any(|&n| self.seen[n].is_some_and(|c| c.owner == Some(rid)));
                if next_to { m.min(self.k.neck_cap) } else { 0 }
            }));
            self.neck_of[r] = map;
        }
        self.neck_buf = buf;
        for (r, map) in self.neck_of.iter_mut().enumerate() {
            if !rivals.get(r).copied().unwrap_or(false) {
                map.clear();
                continue;
            }
            for (n, &m) in self.neck.iter_mut().zip(map.iter()) {
                *n = (*n).max(m);
            }
        }
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
        regions: Option<&Regions>,
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
        let sealed = |t: usize| {
            regions.map_or(0, |r| {
                self.k.region * r.land[t] as i64
                    + self.k.region_enemy * r.enemy[t] as i64
                    + r.troops[t] as i64 * cost_milli / 1000 * self.k.region_troops_pct / 100
                    + self.k.region_village * r.villages[t] as i64
            })
        };
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
                let want_t = self.want(geo, me, t, s);
                let seals = regions.map_or(0, |r| r.land[t]);
                if need > avail {
                    let region_pad = self.k.region_stage > 0 && seals >= self.k.region_stage;
                    if self.k.stage > 0 || region_pad {
                        let full = need.max(want_t.map_or(0, |w| loss + w));
                        let short = full + 1 - troops.min(full + 1);
                        let score = self.capture_gain(geo, me, t, tc, seal) + sealed(t)
                            - loss as i64 * cost_milli / 1000
                            - short as i64 * self.k.short_cost;
                        if blocked.map_or(true, |b| (score, std::cmp::Reverse(t)) > (b.score, std::cmp::Reverse(b.to))) {
                            blocked = Some(Capture { score, from: s, to: t, send: short as u32, sealed: seals });
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
                    if (v, send) > (pick.0, pick.2) {
                        pick = (v, st, send);
                    }
                }
                if sprinting || (self.finishing && tc.owner.is_some() && tc.owner == self.victim) {
                    pick = (1_000_000, 1000, need.max(avail * self.k.send_pct / 100).min(avail));
                }
                let (v, st, send) = pick;
                let region = sealed(t);
                if st < self.k.safe_min && region == 0 {
                    continue;
                }
                let gain = self.capture_gain(geo, me, t, tc, seal);
                let mut shape = 0;
                if self.k.notch > 0 || self.k.thin_pen > 0 {
                    let hugged = geo.nbrs[t].iter().filter(|&&n| self.owned(n, me)).count();
                    if hugged >= 4 {
                        shape = self.k.notch;
                    } else if hugged <= 2 && st < 1000 && want_t.is_some() {
                        shape = -self.k.thin_pen;
                    }
                }
                let kill = if tc.owner.is_some() { tc.troops as i64 * cost_milli / 1000 * self.k.kill_pct / 100 } else { 0 };
                let score = gain * st / 1000 + (v / 1000 - st) * self.k.land / 1000
                    - loss as i64 * cost_milli / 1000
                    + kill
                    + region
                    + shape;
                let better = best.as_ref().map_or(true, |b| {
                    (score, std::cmp::Reverse(t), send)
                        > (b.score, std::cmp::Reverse(b.to), b.send as u64)
                });
                if better {
                    best = Some(Capture { score, from: s, to: t, send: send as u32, sealed: seals });
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
        if let Some(o) = tc.owner {
            gain += self.k.enemy + self.k.enclose * self.enclosure_gain(geo, t, me);
            if self.k.focus > 0 {
                let n = self.land_of.get(o as usize).copied().unwrap_or(0).max(1) as i64;
                gain += self.k.focus * 50 / n;
            }
            if self.k.bld > 0 {
                gain += self.k.bld * self.income_swing(geo, me, t, tc, o);
            }
            if self.victim == Some(o) {
                gain += self.k.victim + if self.finishing { self.k.finish_bonus } else { 0 };
            }
        }
        gain += self.k.hug * hugged - self.k.open * exposed + self.k.height * geo.height[t] as i64;
        gain += self.k.neck_take * self.neck.get(t).copied().unwrap_or(0) as i64;
        if self.k.churn_pen > 0 && self.tick >= self.k.churn_from && self.flipped.get(t).is_some_and(|&f| f > 0 && self.tick < f + self.k.churn_ticks) {
            gain -= self.k.churn_pen;
        }
        if let Some(seal) = seal {
            gain += self.k.seal * seal.cuts[t] as i64;
            if seal.pocket[t] {
                gain -= self.k.backyard;
            }
        }
        gain
    }

    /// The contacted rival with the least known land, kept until it dies, stops touching
    /// us for a while, or someone much smaller turns up.
    fn pick_victim(&mut self, view: &View, geo: &Geometry, mine: &[usize]) {
        let me = view.you;
        let alive = |o: PlayerId| view.public_players.iter().any(|p| p.id == o && p.alive);
        let mut contact = vec![false; self.land_of.len()];
        for &s in mine {
            for &n in &geo.nbrs[s] {
                if let Some(o) = self.seen[n].and_then(|c| c.owner).filter(|&o| o != me) {
                    if let Some(c) = contact.get_mut(o as usize) {
                        *c = true;
                    }
                }
            }
        }
        let best = (0..self.land_of.len())
            .filter(|&o| contact[o] && alive(o as PlayerId) && self.land_of[o] > 0)
            .min_by_key(|&o| (self.land_of[o], o))
            .map(|o| o as PlayerId);
        let tick = view.tick;
        let living = view.public_players.iter().filter(|p| p.alive).count() as u32;
        let board: usize = geo.passable.iter().filter(|&&p| p).count();
        let blitz = (self.k.blitz_alive > 0
            && living <= self.k.blitz_alive
            && view.public_players.len() as u32 > self.k.blitz_alive)
            || (self.k.blitz_share_pct > 0
                && view.public_players.len() > 2
                && mine.len() * 100 >= board * self.k.blitz_share_pct as usize);
        if let Some(v) = self.victim {
            if contact.get(v as usize).copied().unwrap_or(false) {
                self.victim_seen = tick;
            }
            let stale = tick > self.victim_seen + 100 || !alive(v);
            let land = |o: PlayerId| self.land_of.get(o as usize).copied().unwrap_or(0);
            let better = best.is_some_and(|b| b != v && land(b) * 100 <= land(v) * self.k.switch_pct);
            if !stale && !better {
                self.finishing = blitz || self.k.finish_n > 0 && land(v) <= self.k.finish_n;
                return;
            }
        }
        self.victim = best;
        self.victim_seen = tick;
        self.finishing = best.is_some_and(|v| {
            blitz || self.k.finish_n > 0 && self.land_of.get(v as usize).copied().unwrap_or(0) <= self.k.finish_n
        });
    }

    /// Troops a tick the owner of `t` loses and we gain by taking it: its own income, the
    /// boosts it gives the owner's adjacent Villages, and what it would make for us next
    /// to our Farms, Quarries and Villages.
    fn income_swing(&self, geo: &Geometry, me: PlayerId, t: usize, tc: Cell, owner: PlayerId) -> i64 {
        let village_boost = |kind: Building| match kind {
            Building::Farm => self.rules.village.farm_bonus as i64,
            Building::Quarry => self.rules.quarry.village_bonus as i64,
            _ => 0,
        };
        let mut deny = tc.income as i64;
        let mut take = self.rules.base_income as i64;
        match tc.building {
            Some(Building::Village) => {
                take += self.rules.village.bonus as i64;
                for &n in &geo.nbrs[t] {
                    if let Some(c) = self.seen[n].filter(|c| c.owner == Some(me)) {
                        take += c.building.map_or(0, village_boost);
                    }
                }
            }
            Some(b @ (Building::Farm | Building::Quarry)) => {
                let boost = village_boost(b);
                for &n in &geo.nbrs[t] {
                    let Some(c) = self.seen[n] else { continue };
                    if c.building == Some(Building::Village) {
                        if c.owner == Some(owner) {
                            deny += boost;
                        } else if c.owner == Some(me) {
                            take += boost;
                        }
                    }
                }
            }
            _ => {}
        }
        deny + take - 2 * self.rules.base_income as i64
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
        let victim = self.victim;
        let faces_victim = |h: usize| {
            victim.is_some() && geo.nbrs[h].iter().any(|&n| self.seen[n].is_some_and(|c| c.owner == victim))
        };
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
            let mut gain = (Self::safety(have + spare, Some(w)) - Self::safety(have, Some(w)))
                / dist[i] as i64;
            if self.k.vsupply_pct != 100 && faces_victim(origin[i]) {
                gain = gain * self.k.vsupply_pct / 100;
            }
            if best.map_or(true, |b| (gain, std::cmp::Reverse(i)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((gain, i));
            }
        }
        let (gain, i) = best?;
        let troops = self.spare(geo, me, i) as u32;
        Some((gain, Action::Move { from: geo.hexes[i], to: geo.hexes[toward[i]], troops }))
    }

    /// Moves a neighbour's spare troops into the hex we took most recently that is still
    /// short of its garrison, so it isn't simply retaken. Returns the safety gain (milli).
    fn reinforce(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<(i64, Action)> {
        let mut best: Option<(i64, usize, usize, u64)> = None;
        for &f in mine {
            if self.tick >= self.flipped[f] + self.k.reinforce || self.flipped[f] == 0 {
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
                if best.map_or(true, |b| (gain, std::cmp::Reverse(n)) > (b.0, std::cmp::Reverse(b.1))) {
                    best = Some((gain, n, f, spare));
                }
            }
        }
        let (gain, n, f, spare) = best?;
        Some((gain, Action::Move { from: geo.hexes[n], to: geo.hexes[f], troops: spare as u32 }))
    }

    /// A captured economic building on a hex the enemy next to it is about to retake.
    fn doomed_building(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<Action> {
        mine.iter()
            .copied()
            .filter(|&h| self.flipped[h] > 0)
            .filter(|&h| {
                let c = self.seen[h].unwrap();
                matches!(c.building, Some(Building::Village | Building::Farm | Building::Quarry))
                    && geo.nbrs[h].iter().any(|&n| {
                        self.enemy(n, me) && {
                            let stack = self.seen[n].unwrap().troops.saturating_sub(1) as u64;
                            let k = self.kills(stack, self.defense(geo, n, h, c.wall_milli));
                            k * 100 >= c.troops as u64 * self.k.demolish_pct
                        }
                    })
            })
            .min()
            .map(|h| Action::Demolish { at: geo.hexes[h] })
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

impl Bot for StrikeCore {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if let Some(r) = &view.rules {
            self.rules = r.clone();
        }
        self.win = self.rules.attacker_win_milli(view.tick);
        if !self.k.duel_set {
            self.k.duel_set = true;
            if view.public_players.len() == 2 {
                if self.k.duel_late_from > 0 && self.k.late_from > 0 {
                    self.k.late_from = self.k.duel_late_from;
                }
                self.k.late_sprint |= self.k.duel_late_sprint;
            }
        }
        let big = self.k.late_share_milli == 0 || self.my_land * 1000 >= self.k.late_share_milli * self.board_land;
        if self.k.late_from > 0 && view.tick >= self.k.late_from && big {
            self.k.go_late();
        }
        if self.k.cash_from > 0 && view.tick >= self.k.cash_from {
            self.k.safe_min = 0;
        }
        let geo = self.geo.take().unwrap_or_else(|| Geometry::new(view));
        if self.seen.is_empty() {
            self.seen = vec![None; geo.passable.len()];
        }
        if self.board_land == 0 {
            self.board_land = geo.passable.iter().filter(|&&p| p).count().max(1);
        }
        let me = view.you;
        let mut mine = vec![];
        self.tick = view.tick;
        if self.flipped.is_empty() {
            self.flipped = vec![0; geo.passable.len()];
        }
        for c in &view.visible_hexes {
            let Some(i) = geo.slot(c.hex) else { continue };
            if self.seen[i].is_some_and(|old| old.owner != c.owner) {
                self.flipped[i] = view.tick;
            }
            self.seen[i] = Some(Cell {
                owner: c.owner,
                troops: c.troops,
                wall_milli: c.wall_milli,
                building: c.building.as_ref().map(|b| b.kind),
                income: c.income,
            });
            if c.owner == Some(me) {
                mine.push(i);
            }
        }
        self.my_land = mine.len();
        if self.k.focus > 0 || self.k.victim_from > 0 {
            self.land_of = vec![0; 16];
            for o in self.seen.iter().flatten().filter_map(|c| c.owner) {
                if let Some(n) = self.land_of.get_mut(o as usize) {
                    *n += 1;
                }
            }
        }
        if self.k.victim_from > 0 && view.tick >= self.k.victim_from {
            self.pick_victim(view, &geo, &mine);
        }
        let sprinting = view.tick + self.k.sprint >= self.rules.max_ticks
            || (self.k.late_sprint && self.k.late_at.is_some());
        let rush = sprinting || (self.finishing && self.k.finish_rush);
        let seal = (self.k.seal > 0 || self.k.backyard > 0).then(|| self.seal(&geo, me));
        if self.k.neck_take > 0 || self.k.neck_want_pct > 0 {
            self.necks(&geo, me);
        }
        let supply = self.supply(&geo, me, &mine);
        let mut buf = std::mem::take(&mut self.region_buf);
        let use_regions = self.k.region > 0 || self.k.region_enemy > 0;
        if use_regions {
            self.regions(&geo, me, me, &mut buf);
        }
        let (capture, blocked) =
            self.best_capture(&geo, me, &mine, sprinting, seal.as_ref(), use_regions.then_some(&buf));
        self.region_buf = buf;
        let capture = capture.filter(|c| rush || c.score >= self.k.min_score);
        let supply_tick =
            !rush && self.k.supply_every > 0 && view.tick % self.k.supply_every == 0;
        let floor = capture.map_or(self.k.min_score, |c| c.score);
        let staging = blocked
            .filter(|b| {
                let region_pad = self.k.region_stage > 0 && b.sealed >= self.k.region_stage;
                !rush && mine.len() > 1 && (b.score >= floor + self.k.stage || region_pad && b.score > floor)
            })
            .and_then(|b| {
                let contact =
                    mine.iter().any(|&s| geo.nbrs[s].iter().any(|&n| self.enemy(n, me)));
                self.stage_to(&geo, me, b.from, !contact && view.tick < self.k.gather_until)
            });
        let reinforce = (!rush && self.k.reinforce > 0)
            .then(|| self.reinforce(&geo, me, &mine))
            .flatten()
            .filter(|r| r.0 >= self.k.reinforce_min);
        let doomed = (self.k.demolish_pct > 0).then(|| self.doomed_building(&geo, me, &mine)).flatten();
        let action = match (supply, capture) {
            _ if doomed.is_some() => doomed.unwrap(),
            (Some((gain, a)), _) if supply_tick && gain >= self.k.supply_min => a,
            _ if reinforce.is_some() => reinforce.unwrap().1,
            _ if staging.is_some() => staging.unwrap(),
            (_, Some(c)) => {
                Action::Move { from: geo.hexes[c.from], to: geo.hexes[c.to], troops: c.send }
            }
            (Some((gain, a)), None) if gain > 0 => a,
            _ => self.regroup(&geo, me, &mine, view.tick).unwrap_or(Action::NoOp),
        };
        self.geo = Some(geo);
        PlayerCommand::action(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlayerEconomy, Terrain, TerrainKind, VisibleHex};

    /// A radius-4 board where we own the ring at distance 2 from the centre except one
    /// hex, a rival owns the centre, and a mountain sits inside the ring.
    fn ring_view(gap: Hex, mountain: Hex) -> View {
        let radius = 4;
        let w = 2 * radius + 1;
        let mut terrain = vec![None; (w * w) as usize];
        let mut visible = vec![];
        for r in -radius..=radius {
            for q in -radius..=radius {
                let h = Hex::new(q, r);
                if h.distance(Hex::new(0, 0)) > radius {
                    continue;
                }
                let kind = if h == mountain { TerrainKind::Mountain } else { TerrainKind::Open };
                terrain[((r + radius) * w + q + radius) as usize] = Some(Terrain { height: 0, kind });
                let d = h.distance(Hex::new(0, 0));
                let owner = if d == 2 && h != gap { Some(0) } else if d == 0 { Some(1) } else { None };
                visible.push(VisibleHex { hex: h, owner, troops: 5, building: None, income: 1, wall_milli: MILLI });
            }
        }
        View {
            protocol_version: 1,
            rules_version: String::new(),
            tick: 0,
            you: 0,
            radius,
            fog_radius: 3,
            public_players: vec![],
            public_terrain: terrain,
            visible_hexes: visible,
            own_economy: PlayerEconomy { resource: 0 },
            own_queue: vec![],
            visible_events: vec![],
            storm_radius: None,
            rules: None,
        }
    }

    #[test]
    fn closing_a_ring_seals_its_inside_mountains_included() {
        let gap = Hex::new(2, 0);
        let view = ring_view(gap, Hex::new(1, 0));
        let mut core = StrikeCore::new(crate::style("ridgeline").unwrap());
        let geo = Geometry::new(&view);
        core.seen = vec![None; geo.passable.len()];
        for c in &view.visible_hexes {
            let i = geo.slot(c.hex).unwrap();
            core.seen[i] = Some(Cell { owner: c.owner, troops: c.troops, wall_milli: c.wall_milli, building: None, income: 1 });
        }
        let mut r = Regions::default();
        core.regions(&geo, 0, 0, &mut r);
        let g = geo.slot(gap).unwrap();
        // Inside the ring: the centre and five of the six distance-1 hexes are passable.
        assert_eq!(r.land[g], 6);
        assert_eq!(r.enemy[g], 1);
        // Taking a hex outside the ring seals nothing.
        assert_eq!(r.land[geo.slot(Hex::new(3, 0)).unwrap()], 0);
        assert_eq!(r.land[geo.slot(Hex::new(0, 0)).unwrap()], 0);

        // Seen from the centre's owner, the gap is the neck its one hex would fall through.
        core.necks(&geo, 1);
        assert_eq!(core.neck[g], 1);
        assert_eq!(core.neck[geo.slot(Hex::new(3, 0)).unwrap()], 0);
        assert!(core.neck.iter().all(|&n| n <= 1));
    }
}
