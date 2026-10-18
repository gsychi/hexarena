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

/// Starter bot: builds an economy when it can afford one, links Farms to Villages, and
/// otherwise attacks the cheapest adjacent hex it expects to win. Uses only its View.
pub struct Expander {
    rules: RulesConfig,
    seen: HashMap<Hex, VisibleHex>,
    next_kind: usize,
    last_build: u32,
}

impl Default for Expander {
    fn default() -> Self {
        Expander { rules: RulesConfig::default(), seen: HashMap::new(), next_kind: 0, last_build: 0 }
    }
}

impl Expander {
    fn height(view: &View, h: Hex) -> i8 {
        terrain_at(view, h).map(|t| t.height).unwrap_or(0)
    }

    fn management(&self, view: &View, mine: &[&VisibleHex]) -> Vec<ManagementCommand> {
        for farm in mine.iter().filter(|c| matches!(&c.building, Some(b) if b.kind == Building::Farm && b.link_cooldown == 0)) {
            let used = farm.building.as_ref().unwrap().links.len();
            if used >= self.rules.farm.sockets as usize {
                continue;
            }
            let village = mine.iter().find(|v| {
                v.hex.distance(farm.hex) == 1
                    && matches!(&v.building, Some(b) if b.kind == Building::Village && b.links.is_empty())
            });
            if let Some(v) = village {
                return vec![ManagementCommand::SetFarmLink { farm: farm.hex, village: v.hex, enabled: true }];
            }
        }
        let _ = view;
        vec![]
    }

    fn build(&mut self, view: &View, mine: &[&VisibleHex]) -> Option<Action> {
        let buildings = mine.iter().filter(|c| c.building.is_some()).count();
        if buildings >= 8 || view.tick < self.last_build + 12 {
            return None;
        }
        let resource = view.own_economy.resource;
        let kinds = [Building::Farm, Building::Village, Building::Quarry, Building::Wall];
        for off in 0..kinds.len() {
            let kind = kinds[(self.next_kind + off) % kinds.len()];
            let at = mine.iter().filter(|c| c.building.is_none()).find(|c| {
                let h = Self::height(view, c.hex);
                if !self.rules.terrain_allowed(kind, h) {
                    return false;
                }
                let near_quarry = mine.iter().any(|q| {
                    q.hex != c.hex && q.hex.distance(c.hex) <= 1 && matches!(&q.building, Some(b) if b.kind == Building::Quarry)
                });
                let cost = self.rules.build_cost(kind, near_quarry);
                if resource < cost.resource || c.troops < cost.troops + 1 {
                    return false;
                }
                match kind {
                    Building::Village => mine.iter().any(|f| f.hex.distance(c.hex) == 1 && matches!(&f.building, Some(b) if b.kind == Building::Farm)),
                    Building::Quarry => !mine.iter().any(|q| {
                        q.hex.distance(c.hex) < self.rules.quarry.min_friendly_distance
                            && matches!(&q.building, Some(b) if b.kind == Building::Quarry)
                    }),
                    Building::Wall => c.hex.neighbors().iter().any(|n| view.visible_hexes.iter().any(|o| o.hex == *n && o.owner != Some(view.you))),
                    Building::Farm => true,
                }
            });
            if let Some(c) = at {
                self.next_kind = (self.next_kind + off + 1) % kinds.len();
                self.last_build = view.tick;
                return Some(Action::Build { at: c.hex, building: kind });
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
                let Some(t) = terrain_at(view, nb) else { continue };
                if !t.passable() {
                    continue;
                }
                let target = self.seen.get(&nb);
                let (owner, def, wall) = target.map(|v| (v.owner, v.troops, v.wall_milli)).unwrap_or((None, 10, MILLI));
                if owner == Some(me) {
                    continue;
                }
                let d = (self.rules.height_multiplier_milli(from_h, t.height) as u64 * wall as u64 / 1000)
                    .min(self.rules.maximum_total_defense_milli as u64);
                let p = self.rules.attacker_exchange_win_milli as u64;
                let expected_loss = def as u64 * d * (1000 - p) / p / 1000;
                let send = c.troops - 1;
                if (send as u64) < expected_loss * 13 / 10 + 1 {
                    continue;
                }
                let score = -(expected_loss as i64) + if owner.is_some() { 20 } else { 0 };
                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((score, Action::Move { from: c.hex, to: nb, troops: send }));
                }
            }
        }
        best.map(|(_, a)| a)
    }
}

impl Bot for Expander {
    fn name(&self) -> &str {
        "expander"
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        for c in &view.visible_hexes {
            self.seen.insert(c.hex, c.clone());
        }
        let mine: Vec<&VisibleHex> = view.visible_hexes.iter().filter(|c| c.owner == Some(view.you)).collect();
        let management = self.management(view, &mine);
        let action = self
            .build(view, &mine)
            .or_else(|| self.attack(view, &mine))
            .unwrap_or_else(|| {
                let rich = mine.iter().max_by_key(|c| c.troops);
                match rich {
                    Some(c) if view.tick % 8 == 0 && mine.len() > 3 => Action::Gather { target: c.hex },
                    _ => Action::NoOp,
                }
            });
        PlayerCommand { management, action }
    }
}
