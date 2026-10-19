//! Hexarena bot SDK. A bot sees only a fog-filtered `View` and returns a `PlayerCommand`
//! (free management commands plus at most one main action) each tick.

pub mod brains;

pub use hexarena_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};

pub trait Bot {
    fn name(&self) -> &str;
    fn act(&mut self, view: &View) -> PlayerCommand;
}

fn terrain_at(view: &View, h: Hex) -> Option<Terrain> {
    let w = 2 * view.radius + 1;
    if h.q.abs() > view.radius || h.r.abs() > view.radius || (h.q + h.r).abs() > view.radius {
        return None;
    }
    view.public_terrain[((h.r + view.radius) * w + h.q + view.radius) as usize]
}

/// Every tunable number behind a CPU. Serializable so the league can mutate, store and
/// replay any generation. Fields added later must default to the behaviour before they
/// existed, so older genomes keep playing the same way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BotParams {
    /// Attack only when sending at least this % of the expected loss.
    pub attack_margin_pct: u64,
    /// Score bonus for attacking an enemy hex instead of a neutral one.
    pub enemy_bonus: i64,
    /// Stop building once this many buildings stand.
    pub build_cap: usize,
    /// Minimum ticks between builds.
    pub build_every: u32,
    /// Build preference, cycled in order.
    pub kinds: Vec<Building>,
    /// Gather troops toward the army every N ticks.
    pub gather_every: u32,
    /// Score for gaining any hex: it produces troops every tick and counts for placement.
    pub land_value: i64,
    /// Extra score per owned neighbour of a target hex.
    pub hug_value: i64,
    /// Extra score for captures from the focus player.
    pub focus_bonus: i64,
    /// % of a destroyed enemy garrison counted in a capture's score.
    pub kill_value_pct: i64,
    /// Every Nth tick the best small grab from any hex goes before the army (0 = never).
    pub grab_every: u32,
    /// Every Nth gather feeds the hex bordering the most unclaimed land (0 = never).
    pub settle_every: u32,
    /// Whether the army marches on the weakest rival.
    pub march: bool,
    /// Visible enemy stacks of at least this many troops that could take one of our
    /// bordering hexes count as threats (0 = ignore threats; disables the genes below).
    pub threat_min: u32,
    /// Strike the biggest threat from our strongest adjacent hex, before anything else,
    /// when sending at least this % of the expected loss (0 = never).
    pub counter_margin_pct: u64,
    /// Gathers feed the threatened hex: 0 = never, 1 = on the gather cadence,
    /// 2 = every tick while a threat stands.
    pub defend_gather: u32,
    /// The army walks to the threatened hex when it is at most this far away (0 = never).
    pub defend_range: u32,
    /// Over the last N ticks before the cutoff (placement is decided by land), stop
    /// building and spend every action on the cheapest capture available (0 = never).
    pub endgame_ticks: u32,
    /// Endgame captures send this % of the expected loss, so the one action a tick
    /// rarely fails.
    pub endgame_margin_pct: u64,
    /// Endgame score bonus for taking a rival's hex, which also lowers their land.
    pub endgame_enemy_bonus: i64,
    /// Which strategy plays these params; see [`brains`]. Every brain reads the shared
    /// fields it understands and its own [`BotParams::knobs`].
    #[serde(skip_serializing_if = "is_default_brain")]
    pub brain: String,
    /// Brain-specific non-negative integer settings, by name. The league mutates them
    /// like any other gene, so a new brain is tunable without touching the league.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub knobs: BTreeMap<String, i64>,
}

fn is_default_brain(b: &str) -> bool {
    b == brains::DEFAULT
}

impl BotParams {
    /// A brain-specific knob, or `default` when this genome doesn't set it.
    pub fn knob(&self, name: &str, default: i64) -> i64 {
        self.knobs.get(name).copied().unwrap_or(default)
    }
}

impl Default for BotParams {
    fn default() -> Self {
        BotParams {
            attack_margin_pct: 130,
            enemy_bonus: 20,
            build_cap: 8,
            build_every: 12,
            kinds: vec![Building::Farm, Building::Village, Building::Quarry, Building::Wall],
            gather_every: 4,
            land_value: 40,
            hug_value: 8,
            focus_bonus: 40,
            kill_value_pct: 100,
            grab_every: 3,
            settle_every: 2,
            march: true,
            threat_min: 0,
            counter_margin_pct: 0,
            defend_gather: 0,
            defend_range: 0,
            endgame_ticks: 0,
            endgame_margin_pct: 200,
            endgame_enemy_bonus: 20,
            brain: brains::DEFAULT.into(),
            knobs: BTreeMap::new(),
        }
    }
}

/// A visible enemy stack that could capture one of our hexes.
#[derive(Clone, Copy)]
struct Threat {
    stack: Hex,
    /// Our bordering hex the stack can take most cheaply.
    target: Hex,
}

/// A named CPU: a preset of [`BotParams`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Style {
    pub id: String,
    pub name: String,
    pub blurb: String,
    pub params: BotParams,
}

impl Style {
    fn preset(id: &str, name: &str, blurb: &str, params: BotParams) -> Style {
        Style { id: id.into(), name: name.into(), blurb: blurb.into(), params }
    }
}

/// The hand-written CPU roster. All are original to Hexarena.
pub fn roster() -> Vec<Style> {
    use Building::*;
    let base = BotParams::default();
    vec![
        Style::preset(
            "expander",
            "Expander",
            "Balanced. Grabs cheap land and builds a small economy.",
            base.clone(),
        ),
        Style::preset(
            "homesteader",
            "Homesteader",
            "Economy first. Farms, Villages and Quarries everywhere.",
            BotParams {
                attack_margin_pct: 150,
                enemy_bonus: 0,
                build_cap: 18,
                build_every: 6,
                kinds: vec![Farm, Village, Village, Quarry],
                gather_every: 5,
                ..base.clone()
            },
        ),
        Style::preset(
            "raider",
            "Raider",
            "Aggressive. Few buildings, hunts enemy borders.",
            BotParams {
                attack_margin_pct: 112,
                enemy_bonus: 60,
                build_cap: 3,
                build_every: 30,
                kinds: vec![Village, Farm],
                gather_every: 3,
                ..base.clone()
            },
        ),
        Style::preset(
            "warden",
            "Warden",
            "Defensive. Linked Walls on the border, Quarries on hills.",
            BotParams {
                attack_margin_pct: 170,
                enemy_bonus: 10,
                build_cap: 14,
                build_every: 9,
                kinds: vec![Wall, Quarry, Wall, Farm, Village],
                gather_every: 6,
                ..base
            },
        ),
    ]
}

pub fn style(id: &str) -> Option<Style> {
    roster().into_iter().find(|s| s.id == id)
}

/// Rule-based CPU player driven by a [`Style`]. Uses only its View.
pub struct Expander {
    name: String,
    p: BotParams,
    rules: RulesConfig,
    seen: Memory,
    next_kind: usize,
    last_build: u32,
    /// Border hex that troops are being gathered toward for the next push.
    staging: Option<Hex>,
    /// The opponent everyone should be finishing off; see [`Expander::focus`].
    focus_player: Option<PlayerId>,
    /// This tick's most dangerous enemy stack on our border, if threats are watched.
    threat: Option<Threat>,
}

impl Default for Expander {
    fn default() -> Self {
        Self::new(roster().remove(0))
    }
}

/// The last seen state of every hex, in a flat grid laid out like `public_terrain`.
#[derive(Default)]
struct Memory {
    radius: i32,
    cells: Vec<Option<VisibleHex>>,
}

impl Memory {
    fn slot(&self, h: &Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * (2 * r + 1) + h.q + r) as usize)
    }

    fn get(&self, h: &Hex) -> Option<&VisibleHex> {
        self.slot(h).and_then(|i| self.cells.get(i)?.as_ref())
    }

    fn insert(&mut self, radius: i32, c: &VisibleHex) {
        if self.cells.is_empty() {
            self.radius = radius;
            self.cells = vec![None; ((2 * radius + 1) * (2 * radius + 1)) as usize];
        }
        let Some(i) = self.slot(&c.hex) else { return };
        match &mut self.cells[i] {
            Some(old) => old.clone_from(c),
            empty => *empty = Some(c.clone()),
        }
    }

    fn values(&self) -> impl Iterator<Item = &VisibleHex> {
        self.cells.iter().flatten()
    }
}

fn is_kind(c: &VisibleHex, k: Building) -> bool {
    matches!(&c.building, Some(b) if b.kind == k)
}

impl Expander {
    pub fn new(style: Style) -> Self {
        Self::with_params(style.id, style.params)
    }

    pub fn with_params(name: impl Into<String>, p: BotParams) -> Self {
        Expander {
            name: name.into(),
            p,
            rules: RulesConfig::default(),
            seen: Memory::default(),
            next_kind: 0,
            last_build: 0,
            staging: None,
            focus_player: None,
            threat: None,
        }
    }

    fn height(view: &View, h: Hex) -> i8 {
        terrain_at(view, h).map(|t| t.height).unwrap_or(0)
    }

    fn management(&self, mine: &[&VisibleHex]) -> Vec<ManagementCommand> {
        let mut out = vec![];
        for farm in mine.iter().filter(
            |c| matches!(&c.building, Some(b) if b.kind == Building::Farm && b.link_cooldown == 0),
        ) {
            let used = farm.building.as_ref().unwrap().links.len();
            if used >= self.rules.farm.sockets as usize {
                continue;
            }
            let village = mine.iter().find(|v| {
                v.hex.distance(farm.hex) == 1
                    && matches!(&v.building, Some(b) if b.kind == Building::Village && b.links.is_empty())
            });
            if let Some(v) = village {
                out.push(ManagementCommand::SetFarmLink {
                    farm: farm.hex,
                    village: v.hex,
                    enabled: true,
                });
                break;
            }
        }
        let max = self.rules.wall.max_links as usize;
        let open_wall = |c: &&&VisibleHex| matches!(&c.building, Some(b) if b.kind == Building::Wall && b.link_cooldown == 0 && b.links.len() < max);
        'walls: for a in mine.iter().filter(open_wall) {
            for b in mine.iter().filter(open_wall) {
                let linked = a.building.as_ref().unwrap().links.contains(&b.hex);
                if a.hex.distance(b.hex) == 1 && !linked {
                    out.push(ManagementCommand::SetWallLink {
                        wall_a: a.hex,
                        wall_b: b.hex,
                        enabled: true,
                    });
                    break 'walls;
                }
            }
        }
        out
    }

    fn build(&mut self, view: &View, mine: &[&VisibleHex]) -> Option<Action> {
        let buildings = mine.iter().filter(|c| c.building.is_some()).count();
        if buildings >= self.p.build_cap || view.tick < self.last_build + self.p.build_every
        {
            return None;
        }
        let resource = view.own_economy.resource;
        let kinds = self.p.kinds.clone();
        if kinds.is_empty() {
            return None;
        }
        for off in 0..kinds.len() {
            let kind = kinds[(self.next_kind + off) % kinds.len()];
            let at = mine.iter().filter(|c| c.building.is_none()).find(|c| {
                let h = Self::height(view, c.hex);
                if !self.rules.terrain_allowed(kind, h) {
                    return false;
                }
                let near_quarry = mine.iter().any(|q| {
                    q.hex != c.hex && q.hex.distance(c.hex) <= 1 && is_kind(q, Building::Quarry)
                });
                let cost = self.rules.build_cost(kind, near_quarry);
                if resource < cost.resource || c.troops < cost.troops + 1 {
                    return false;
                }
                let border = c.hex.neighbors().iter().any(|n| {
                    view.visible_hexes
                        .iter()
                        .any(|o| o.hex == *n && o.owner != Some(view.you))
                });
                match kind {
                    Building::Village => mine
                        .iter()
                        .any(|f| f.hex.distance(c.hex) == 1 && is_kind(f, Building::Farm)),
                    Building::Quarry => !mine.iter().any(|q| {
                        q.hex.distance(c.hex) < self.rules.quarry.min_friendly_distance
                            && is_kind(q, Building::Quarry)
                    }),
                    Building::Wall => border,
                    Building::Farm => true,
                }
            });
            if let Some(c) = at {
                self.next_kind = (self.next_kind + off + 1) % kinds.len();
                self.last_build = view.tick;
                return Some(Action::Build {
                    at: c.hex,
                    building: kind,
                });
            }
        }
        None
    }

    /// Expected attacker losses to take `target` from a hex at height `from_h`.
    fn capture_cost(&self, view: &View, from_h: i8, target: Hex) -> Option<(u64, bool)> {
        let t = terrain_at(view, target).filter(|t| t.passable())?;
        let (owner, def, wall) = self
            .seen
            .get(&target)
            .map(|v| (v.owner, v.troops, v.wall_milli))
            .unwrap_or((None, 10, MILLI));
        if owner == Some(view.you) {
            return None;
        }
        Some((self.fight_loss(from_h, t.height, def, wall), owner.is_some()))
    }

    /// Expected attacker losses against `def` troops at height `to_h` behind `wall`.
    fn fight_loss(&self, from_h: i8, to_h: i8, def: u32, wall: u32) -> u64 {
        let d = (self.rules.height_multiplier_milli(from_h, to_h) as u64 * wall as u64 / 1000)
            .min(self.rules.maximum_total_defense_milli as u64);
        let p = self.rules.attacker_exchange_win_milli as u64;
        def as u64 * d * (1000 - p) / p / 1000
    }

    /// The visible enemy stack with the most troops to spare after taking one of our
    /// bordering hexes. Only enemy hexes are scanned, each against its six neighbours.
    fn find_threat(&self, view: &View) -> Option<Threat> {
        if self.p.threat_min == 0 {
            return None;
        }
        let mut best: Option<(u64, Threat)> = None;
        for e in &view.visible_hexes {
            if e.owner.map_or(true, |o| o == view.you) || e.troops < self.p.threat_min {
                continue;
            }
            let eh = Self::height(view, e.hex);
            let weakest = e
                .hex
                .neighbors()
                .into_iter()
                .filter_map(|n| {
                    let v = self.seen.get(&n).filter(|v| v.owner == Some(view.you))?;
                    Some((self.fight_loss(eh, Self::height(view, n), v.troops, v.wall_milli), n))
                })
                .min();
            let Some((loss, target)) = weakest else { continue };
            let spare = e.troops as u64;
            if spare <= loss {
                continue;
            }
            let excess = spare - loss;
            if best.as_ref().map_or(true, |(b, _)| excess > *b) {
                best = Some((excess, Threat { stack: e.hex, target }));
            }
        }
        best.map(|(_, t)| t)
    }

    /// Crushes the threatening stack from our strongest hex next to it, if affordable.
    fn counter(&self, view: &View, t: Threat) -> Option<Action> {
        let m = self.p.counter_margin_pct;
        if m == 0 {
            return None;
        }
        let from = t
            .stack
            .neighbors()
            .into_iter()
            .filter_map(|n| self.seen.get(&n).filter(|v| v.owner == Some(view.you)))
            .max_by_key(|v| (v.troops, std::cmp::Reverse(v.hex)))?;
        if from.troops < 2 {
            return None;
        }
        let send = from.troops - 1;
        let (loss, _) = self.capture_cost(view, Self::height(view, from.hex), t.stack)?;
        ((send as u64) >= loss * m / 100 + 1)
            .then_some(Action::Move { from: from.hex, to: t.stack, troops: send })
    }

    /// The best affordable capture from `c`, sending everything but one troop.
    /// Captures that touch more of our land score higher, so growth stays compact and
    /// pockets of enemy or neutral land get filled in (and enclosed) instead of skipped.
    fn best_capture(&self, view: &View, c: &VisibleHex) -> Option<(i64, Action)> {
        if c.troops < 2 {
            return None;
        }
        let send = c.troops - 1;
        let from_h = Self::height(view, c.hex);
        let mut best: Option<(i64, Action)> = None;
        for nb in c.hex.neighbors() {
            let Some((loss, enemy)) = self.capture_cost(view, from_h, nb) else {
                continue;
            };
            if (send as u64) < loss * self.p.attack_margin_pct / 100 + 1 {
                continue;
            }
            let hugged = nb
                .neighbors()
                .iter()
                .filter(|n| self.seen.get(n).is_some_and(|v| v.owner == Some(view.you)))
                .count() as i64;
            let def = self.seen.get(&nb).map_or(0, |v| v.troops as i64);
            // Enemy troops destroyed count in our favour, so a bigger stack
            // is willing to crush a smaller one instead of dodging it.
            let score = self.p.land_value + self.p.hug_value * hugged - loss as i64
                + if enemy { self.p.enemy_bonus + def * self.p.kill_value_pct / 100 } else { 0 }
                + if enemy && self.seen.get(&nb).and_then(|v| v.owner) == self.focus_player {
                    self.p.focus_bonus
                } else {
                    0
                };
            if best.as_ref().map_or(true, |(s, _)| score > *s) {
                let action = Action::Move { from: c.hex, to: nb, troops: send };
                best = Some((score, action));
            }
        }
        best
    }

    /// One step from the army toward the nearest owned hex that borders land we could take.
    fn advance(&self, view: &View, army: &VisibleHex) -> Option<Action> {
        let owned = |h: &Hex| self.seen.get(h).is_some_and(|v| v.owner == Some(view.you));
        let frontier = |h: Hex| {
            h.neighbors()
                .iter()
                .any(|n| !owned(n) && terrain_at(view, *n).is_some_and(|t| t.passable()))
        };
        if frontier(army.hex) {
            return None;
        }
        Self::step_toward(army, frontier, |h| owned(&h))
    }

    /// Moves the whole army one hex along the shortest path (through hexes accepted by
    /// `through`) to the nearest hex satisfying `goal`. Ties follow neighbour order.
    fn step_toward(
        army: &VisibleHex,
        goal: impl Fn(Hex) -> bool,
        through: impl Fn(Hex) -> bool,
    ) -> Option<Action> {
        let mut first_step = HashMap::from([(army.hex, army.hex)]);
        let mut queue = VecDeque::from([army.hex]);
        while let Some(h) = queue.pop_front() {
            if h != army.hex && goal(h) {
                let to = first_step[&h];
                return Some(Action::Move { from: army.hex, to, troops: army.troops - 1 });
            }
            for n in h.neighbors() {
                if through(n) && !first_step.contains_key(&n) {
                    let step = if h == army.hex { n } else { first_step[&h] };
                    first_step.insert(n, step);
                    queue.push_back(n);
                }
            }
        }
        None
    }

    /// The living opponent we have seen the least land of: everyone piling onto the
    /// weakest player is what lets free-for-alls finish instead of stalemating.
    fn focus(&self, view: &View) -> Option<PlayerId> {
        let mut land: HashMap<PlayerId, usize> = HashMap::new();
        for v in self.seen.values() {
            if let Some(o) = v.owner.filter(|&o| o != view.you) {
                *land.entry(o).or_default() += 1;
            }
        }
        view.public_players
            .iter()
            .filter(|p| p.alive && p.id != view.you)
            .filter_map(|p| land.get(&p.id).map(|&n| (n, p.id)))
            .min()
            .map(|(_, id)| id)
    }

    /// Marches the army toward the focus player's nearest known hex, as long as the
    /// next step is a capture it can afford.
    fn march(&self, view: &View, army: &VisibleHex, focus: PlayerId) -> Option<Action> {
        let passable = |h: Hex| terrain_at(view, h).is_some_and(|t| t.passable());
        let theirs = |h: Hex| self.seen.get(&h).is_some_and(|v| v.owner == Some(focus));
        let step = Self::step_toward(army, theirs, passable)?;
        let Action::Move { to, troops, .. } = step else {
            return None;
        };
        let affordable = self
            .capture_cost(view, Self::height(view, army.hex), to)
            .map_or(true, |(loss, _)| troops as u64 > loss * self.p.attack_margin_pct / 100);
        affordable.then_some(step)
    }

    /// The army is our biggest stack. It marches on the focus player, captures, or heads
    /// for the border. On the style's gather cadence the rest of our land feeds troops to
    /// it instead, alternating with the hex bordering the most unclaimed land so neutral
    /// ground keeps getting settled.
    fn army_action(&mut self, view: &View, mine: &[&VisibleHex]) -> Option<Action> {
        let army = *mine.iter().max_by_key(|c| (c.troops, std::cmp::Reverse(c.hex)))?;
        if army.troops < 2 {
            return None;
        }
        let every = self.p.gather_every.max(1);
        let gather_tick = view.tick % every == 0;
        let defend = self.threat.filter(|_| {
            self.p.defend_gather >= 2 || (self.p.defend_gather == 1 && gather_tick)
        });
        if let (Some(t), true) = (defend, mine.len() > 3) {
            self.staging = Some(t.target);
            return Some(Action::Gather { target: t.target });
        }
        if mine.len() > 3 && gather_tick {
            let n = self.p.settle_every;
            let settle = n > 0 && (view.tick / every) % n == n - 1;
            let target = settle
                .then(|| self.settlement(view, mine))
                .flatten()
                .unwrap_or(army.hex);
            self.staging = Some(target);
            return Some(Action::Gather { target });
        }
        if let Some(t) = self.threat.filter(|t| {
            let d = army.hex.distance(t.target);
            d > 0 && d <= self.p.defend_range as i32 && army.hex.distance(t.stack) > 1
        }) {
            let owned = |h: Hex| self.seen.get(&h).is_some_and(|v| v.owner == Some(view.you));
            if let Some(step) = Self::step_toward(army, |h| h == t.target, owned) {
                return Some(step);
            }
        }
        self.focus_player
            .filter(|_| self.p.march)
            .and_then(|f| self.march(view, army, f))
            .or_else(|| self.best_capture(view, army).map(|(_, a)| a))
            .or_else(|| self.advance(view, army))
    }

    /// The owned hex bordering the most unclaimed land, where settlers gather.
    fn settlement(&self, view: &View, mine: &[&VisibleHex]) -> Option<Hex> {
        let neutral = |h: &Hex| {
            terrain_at(view, *h).is_some_and(|t| t.passable())
                && self.seen.get(h).map_or(true, |v| v.owner.is_none())
        };
        mine.iter()
            .map(|c| (c.hex.neighbors().iter().filter(|n| neutral(n)).count(), c.troops, c.hex))
            .filter(|(n, _, _)| *n > 0)
            .max_by_key(|&(n, t, h)| (n, t, std::cmp::Reverse(h)))
            .map(|(_, _, h)| h)
    }

    /// Land race before the cutoff: one action a tick is the bottleneck, so take the
    /// best-scoring affordable hex from any of our hexes, sending enough to be near
    /// certain. With nothing affordable, gather toward the border hex facing the
    /// cheapest target.
    fn endgame(&self, view: &View, mine: &[&VisibleHex]) -> Option<Action> {
        let owned = |h: &Hex| self.seen.get(h).is_some_and(|v| v.owner == Some(view.you));
        let mut best: Option<(i64, Action)> = None;
        let mut cheapest: Option<(u64, std::cmp::Reverse<u32>, Hex)> = None;
        for c in mine {
            let from_h = Self::height(view, c.hex);
            for nb in c.hex.neighbors() {
                let Some((loss, enemy)) = self.capture_cost(view, from_h, nb) else {
                    continue;
                };
                let key = (loss, std::cmp::Reverse(c.troops), c.hex);
                if cheapest.map_or(true, |k| key < k) {
                    cheapest = Some(key);
                }
                let send = (loss * self.p.endgame_margin_pct / 100 + 1).max(1);
                if c.troops < 2 || send > (c.troops - 1) as u64 || send < loss + 1 {
                    continue;
                }
                let hugged = nb.neighbors().iter().filter(|n| owned(n)).count() as i64;
                let score = self.p.hug_value * hugged - loss as i64
                    + if enemy { self.p.endgame_enemy_bonus } else { 0 };
                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((score, Action::Move { from: c.hex, to: nb, troops: send as u32 }));
                }
            }
        }
        best.map(|(_, a)| a).or_else(|| {
            let (_, _, at) = cheapest?;
            (mine.len() > 1).then_some(Action::Gather { target: at })
        })
    }

    /// Cheap grabs from any hex, used when the army has nothing to do.
    fn opportunistic(&self, view: &View, mine: &[&VisibleHex]) -> Option<Action> {
        mine.iter()
            .filter_map(|c| self.best_capture(view, c))
            .max_by_key(|(s, _)| *s)
            .map(|(_, a)| a)
    }
}

impl Bot for Expander {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        for c in &view.visible_hexes {
            self.seen.insert(view.radius, c);
        }
        let mine: Vec<&VisibleHex> = view
            .visible_hexes
            .iter()
            .filter(|c| c.owner == Some(view.you))
            .collect();
        let management = self.management(&mine);
        self.focus_player = self.focus(view);
        let e = self.p.endgame_ticks;
        if e > 0 && view.tick + e >= self.rules.max_ticks {
            let action = self.endgame(view, &mine).unwrap_or(Action::NoOp);
            return PlayerCommand { management, action };
        }
        self.threat = self.find_threat(view);
        let g = self.p.grab_every;
        let grab_tick = g > 0 && view.tick % g == 1 % g;
        let action = self
            .threat
            .and_then(|t| self.counter(view, t))
            .or_else(|| self.build(view, &mine))
            .or_else(|| grab_tick.then(|| self.opportunistic(view, &mine)).flatten())
            .or_else(|| self.army_action(view, &mine))
            .or_else(|| self.opportunistic(view, &mine))
            .unwrap_or(Action::NoOp);
        PlayerCommand { management, action }
    }
}
