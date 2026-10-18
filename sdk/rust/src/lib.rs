//! Hexarena bot SDK. A bot sees only a fog-filtered `View` and returns a `PlayerCommand`
//! (free management commands plus at most one main action) each tick.

pub use hexarena_protocol::*;
use std::collections::HashMap;

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
        gather_every: 8,
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
        gather_every: 10,
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
        gather_every: 5,
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
        gather_every: 12,
    },
];

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

    fn attack(&self, view: &View, mine: &[&VisibleHex]) -> Option<Action> {
        let me = view.you;
        let mut best: Option<(i64, Action)> = None;
        for c in mine {
            if c.troops < 2 {
                continue;
            }
            let from_h = Self::height(view, c.hex);
            for nb in c.hex.neighbors() {
                let Some(t) = terrain_at(view, nb) else {
                    continue;
                };
                if !t.passable() {
                    continue;
                }
                let target = self.seen.get(&nb);
                let (owner, def, wall) = target
                    .map(|v| (v.owner, v.troops, v.wall_milli))
                    .unwrap_or((None, 10, MILLI));
                if owner == Some(me) {
                    continue;
                }
                let d = (self.rules.height_multiplier_milli(from_h, t.height) as u64 * wall as u64
                    / 1000)
                    .min(self.rules.maximum_total_defense_milli as u64);
                let p = self.rules.attacker_exchange_win_milli as u64;
                let expected_loss = def as u64 * d * (1000 - p) / p / 1000;
                let send = c.troops - 1;
                if (send as u64) < expected_loss * self.style.attack_margin_pct / 100 + 1 {
                    continue;
                }
                let score = -(expected_loss as i64)
                    + if owner.is_some() {
                        self.style.enemy_bonus
                    } else {
                        0
                    };
                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((
                        score,
                        Action::Move {
                            from: c.hex,
                            to: nb,
                            troops: send,
                        },
                    ));
                }
            }
        }
        best.map(|(_, a)| a)
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
        let action = self
            .build(view, &mine)
            .or_else(|| self.attack(view, &mine))
            .unwrap_or_else(|| {
                let rich = mine.iter().max_by_key(|c| c.troops);
                match rich {
                    Some(c) if view.tick % self.style.gather_every == 0 && mine.len() > 3 => {
                        Action::Gather { target: c.hex }
                    }
                    _ => Action::NoOp,
                }
            });
        PlayerCommand { management, action }
    }
}
