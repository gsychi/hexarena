//! Hexarena bot SDK. A bot sees only a fog-filtered `View` and returns a `PlayerCommand`
//! (free management commands plus at most one main action) each tick.

pub use hexarena_protocol::*;
use std::collections::{HashMap, VecDeque};

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

/// Tunable behaviour for the built-in CPU players.
#[derive(Clone, Debug)]
pub struct Style {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    /// Attack only when sending at least this % of the expected loss.
    pub attack_margin_pct: u64,
    /// Score bonus for attacking an enemy hex instead of a neutral one.
    pub enemy_bonus: i64,
    /// Stop building once this many buildings stand.
    pub build_cap: usize,
    /// Minimum ticks between builds.
    pub build_every: u32,
    /// Build preference, cycled in order.
    pub kinds: &'static [Building],
    /// Gather troops toward the richest hex every N ticks.
    pub gather_every: u32,
}

/// The built-in CPU roster. All are original to Hexarena.
pub const ROSTER: [Style; 4] = [
    Style {
        id: "expander",
        name: "Expander",
        blurb: "Balanced. Grabs cheap land and builds a small economy.",
        attack_margin_pct: 130,
        enemy_bonus: 20,
        build_cap: 8,
        build_every: 12,
        kinds: &[
            Building::Farm,
            Building::Village,
            Building::Quarry,
            Building::Wall,
        ],
        gather_every: 4,
    },
    Style {
        id: "homesteader",
        name: "Homesteader",
        blurb: "Economy first. Farms, Villages and Quarries everywhere.",
        attack_margin_pct: 150,
        enemy_bonus: 0,
        build_cap: 18,
        build_every: 6,
        kinds: &[
            Building::Farm,
            Building::Village,
            Building::Village,
            Building::Quarry,
        ],
        gather_every: 5,
    },
    Style {
        id: "raider",
        name: "Raider",
        blurb: "Aggressive. Few buildings, hunts enemy borders.",
        attack_margin_pct: 112,
        enemy_bonus: 60,
        build_cap: 3,
        build_every: 30,
        kinds: &[Building::Village, Building::Farm],
        gather_every: 3,
    },
    Style {
        id: "warden",
        name: "Warden",
        blurb: "Defensive. Linked Walls on the border, Quarries on hills.",
        attack_margin_pct: 170,
        enemy_bonus: 10,
        build_cap: 14,
        build_every: 9,
        kinds: &[
            Building::Wall,
            Building::Quarry,
            Building::Wall,
            Building::Farm,
            Building::Village,
        ],
        gather_every: 6,
    },
];

/// Score for gaining any hex: it produces troops every tick and counts for placement.
const LAND_VALUE: i64 = 40;
/// Extra score per owned neighbour of a target hex.
const HUG_VALUE: i64 = 8;
/// Extra score for captures from the focus player.
const FOCUS_BONUS: i64 = 40;
/// Every Nth tick the best small grab from any hex goes before the army's move.
const GRAB_EVERY: u32 = 3;

pub fn style(id: &str) -> Option<&'static Style> {
    ROSTER.iter().find(|s| s.id == id)
}

/// Rule-based CPU player driven by a [`Style`]. Uses only its View.
pub struct Expander {
    style: Style,
    rules: RulesConfig,
    seen: HashMap<Hex, VisibleHex>,
    next_kind: usize,
    last_build: u32,
    /// Border hex that troops are being gathered toward for the next push.
    staging: Option<Hex>,
    /// The opponent everyone should be finishing off; see [`Expander::focus`].
    focus_player: Option<PlayerId>,
}

impl Default for Expander {
    fn default() -> Self {
        Self::new(ROSTER[0].clone())
    }
}

fn is_kind(c: &VisibleHex, k: Building) -> bool {
    matches!(&c.building, Some(b) if b.kind == k)
}

impl Expander {
    pub fn new(style: Style) -> Self {
        Expander {
            style,
            rules: RulesConfig::default(),
            seen: HashMap::new(),
            next_kind: 0,
            last_build: 0,
            staging: None,
            focus_player: None,
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
        if buildings >= self.style.build_cap || view.tick < self.last_build + self.style.build_every
        {
            return None;
        }
        let resource = view.own_economy.resource;
        let kinds = self.style.kinds;
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
        let d = (self.rules.height_multiplier_milli(from_h, t.height) as u64 * wall as u64 / 1000)
            .min(self.rules.maximum_total_defense_milli as u64);
        let p = self.rules.attacker_exchange_win_milli as u64;
        Some((def as u64 * d * (1000 - p) / p / 1000, owner.is_some()))
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
            if (send as u64) < loss * self.style.attack_margin_pct / 100 + 1 {
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
            let score = LAND_VALUE + HUG_VALUE * hugged - loss as i64
                + if enemy { self.style.enemy_bonus + def } else { 0 }
                + if enemy && self.seen.get(&nb).and_then(|v| v.owner) == self.focus_player {
                    FOCUS_BONUS
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
            .map_or(true, |(loss, _)| troops as u64 > loss * self.style.attack_margin_pct / 100);
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
        let every = self.style.gather_every;
        if mine.len() > 3 && view.tick % every == 0 {
            let settle = (view.tick / every) % 2 == 1;
            let target = settle
                .then(|| self.settlement(view, mine))
                .flatten()
                .unwrap_or(army.hex);
            self.staging = Some(target);
            return Some(Action::Gather { target });
        }
        self.focus_player
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
        self.style.id
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        for c in &view.visible_hexes {
            self.seen.insert(c.hex, c.clone());
        }
        let mine: Vec<&VisibleHex> = view
            .visible_hexes
            .iter()
            .filter(|c| c.owner == Some(view.you))
            .collect();
        let management = self.management(&mine);
        self.focus_player = self.focus(view);
        let grab_tick = view.tick % GRAB_EVERY == 1;
        let action = self
            .build(view, &mine)
            .or_else(|| grab_tick.then(|| self.opportunistic(view, &mine)).flatten())
            .or_else(|| self.army_action(view, &mine))
            .or_else(|| self.opportunistic(view, &mine))
            .unwrap_or(Action::NoOp);
        PlayerCommand { management, action }
    }
}
