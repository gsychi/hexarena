//! Grazer: Relay's proven capture/relay executor with a geometry-aware troop economy.
//!
//! Villages occupy one colour of a hex three-colouring and Farms the other two, so a
//! mature Village can touch six Farms while each Farm boosts up to three Villages. An
//! optional Quarry plan interleaves hill Quarries before the lattice for discounts and
//! the live-rule Village adjacency bonus. Construction is restricted by owned-path depth;
//! every non-build tick is delegated unchanged to Relay.

use super::relay::Relay;
use crate::{Bot, Style};
use hexarena_protocol::{Action, Building, Hex, PlayerCommand, RulesConfig, View};
use std::collections::VecDeque;

struct Knobs {
    mode: i64,
    start: u32,
    stop: u32,
    every: u32,
    cap: usize,
    quarry_cap: usize,
    land_min: usize,
    depth: u32,
    keep: u32,
}

impl Knobs {
    fn read(style: &Style) -> Self {
        let k = |name: &str, default: i64| style.params.knob(name, default).max(0);
        Self {
            mode: k("economy_mode", 1),
            start: k("economy_start", 500) as u32,
            stop: k("economy_stop", 5000) as u32,
            every: k("economy_every", 6).max(1) as u32,
            cap: k("economy_cap", 120) as usize,
            quarry_cap: k("economy_quarry_cap", 4) as usize,
            land_min: k("economy_land_min", 45) as usize,
            depth: k("economy_depth", 2) as u32,
            keep: k("economy_keep", 20) as u32,
        }
    }
}

pub struct Grazer {
    name: String,
    inner: Relay,
    rules: RulesConfig,
    k: Knobs,
    last_build: Option<u32>,
    builds: usize,
}

impl Grazer {
    pub fn new(style: Style) -> Self {
        Self {
            name: style.id.clone(),
            inner: Relay::new(style.clone()),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            last_build: None,
            builds: 0,
        }
    }

    fn site(&self, view: &View, busy: Option<Hex>) -> Option<(Hex, Building)> {
        if self.k.mode == 0
            || self.builds >= self.k.cap
            || view.tick < self.k.start
            || view.tick >= self.k.stop
            || self
                .last_build
                .is_some_and(|tick| view.tick < tick + self.k.every)
        {
            return None;
        }
        let radius = view.radius;
        let width = (2 * radius + 1) as usize;
        let slots = width * width;
        let slot = |hex: Hex| {
            (hex.q.abs() <= radius && hex.r.abs() <= radius && (hex.q + hex.r).abs() <= radius)
                .then(|| (hex.r + radius) as usize * width + (hex.q + radius) as usize)
        };
        let hexes: Vec<Hex> = (0..slots)
            .map(|i| Hex::new((i % width) as i32 - radius, (i / width) as i32 - radius))
            .collect();
        let passable = |i: usize| {
            view.public_terrain
                .get(i)
                .copied()
                .flatten()
                .is_some_and(|terrain| terrain.passable())
        };
        let mut own = vec![false; slots];
        let mut troops = vec![0u32; slots];
        let mut building = vec![None; slots];
        for cell in &view.visible_hexes {
            let Some(i) = slot(cell.hex) else {
                continue;
            };
            own[i] = cell.owner == Some(view.you);
            troops[i] = cell.troops;
            building[i] = cell.building.as_ref().map(|building| building.kind);
        }
        if own.iter().filter(|owned| **owned).count() < self.k.land_min {
            return None;
        }
        let neighbors: Vec<Vec<usize>> = hexes
            .iter()
            .map(|hex| {
                hex.neighbors()
                    .into_iter()
                    .filter_map(slot)
                    .filter(|&i| passable(i))
                    .collect()
            })
            .collect();
        let mut depth = vec![u32::MAX; slots];
        let mut queue = VecDeque::new();
        for i in 0..slots {
            if own[i] && neighbors[i].iter().any(|&n| !own[n]) {
                depth[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            for &n in &neighbors[i] {
                if own[n] && depth[n] == u32::MAX {
                    depth[n] = depth[i] + 1;
                    queue.push_back(n);
                }
            }
        }
        let villages = building
            .iter()
            .filter(|building| **building == Some(Building::Village))
            .count();
        let quarries = building
            .iter()
            .filter(|building| **building == Some(Building::Quarry))
            .count();
        let mut best: Option<(i64, usize, Building)> = None;
        for i in 0..slots {
            if !own[i] || building[i].is_some() || depth[i] < self.k.depth || busy == Some(hexes[i])
            {
                continue;
            }
            let height = view.public_terrain[i].map_or(0, |terrain| terrain.height);
            let color = (hexes[i].q - hexes[i].r).rem_euclid(3);
            let count_adjacent = |kind: Building| {
                neighbors[i]
                    .iter()
                    .filter(|&&n| own[n] && building[n] == Some(kind))
                    .count() as i64
            };
            let adjacent_villages = count_adjacent(Building::Village);
            let adjacent_farms = count_adjacent(Building::Farm);
            let adjacent_quarries = count_adjacent(Building::Quarry);
            let potential_farms = neighbors[i]
                .iter()
                .filter(|&&n| {
                    own[n]
                        && building[n].is_none()
                        && view.public_terrain[n].is_some_and(|terrain| {
                            self.rules.terrain_allowed(Building::Farm, terrain.height)
                        })
                })
                .count() as i64;
            let choices: &[Building] = match self.k.mode {
                1 => &[Building::Farm, Building::Village],
                2 => &[Building::Farm, Building::Village, Building::Quarry],
                3 => &[Building::Village],
                _ => &[Building::Farm, Building::Village],
            };
            for &kind in choices {
                if matches!(self.k.mode, 1 | 2)
                    && ((kind == Building::Village && color != 0)
                        || (kind == Building::Farm && color == 0))
                {
                    continue;
                }
                if !self.rules.terrain_allowed(kind, height) {
                    continue;
                }
                if kind == Building::Farm && adjacent_villages == 0 {
                    continue;
                }
                if kind == Building::Quarry {
                    if quarries >= self.k.quarry_cap || villages < quarries {
                        continue;
                    }
                    let too_close = (0..slots).any(|q| {
                        own[q]
                            && building[q] == Some(Building::Quarry)
                            && hexes[i].distance(hexes[q]) < self.rules.quarry.min_friendly_distance
                    });
                    if too_close {
                        continue;
                    }
                }
                let discounted = adjacent_quarries > 0;
                let cost = self.rules.build_cost(kind, discounted);
                if troops[i] <= cost.troops + self.k.keep {
                    continue;
                }
                let score = match kind {
                    Building::Farm => 1600 * adjacent_villages + if discounted { 100 } else { 0 },
                    Building::Village => {
                        900 + adjacent_farms * 160
                            + potential_farms * 35
                            + if discounted { 180 } else { 0 }
                    }
                    Building::Quarry => 1900 + depth[i] as i64 * 20 + height as i64 * 20,
                    Building::Wall => 0,
                };
                if best.is_none_or(|current| score > current.0) {
                    best = Some((score, i, kind));
                }
            }
        }
        best.map(|(_, i, kind)| (hexes[i], kind))
    }
}

impl Bot for Grazer {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if let Some(rules) = &view.rules {
            self.rules = rules.clone();
        }
        let mut command = self.inner.act(view);
        let busy = match command.action {
            Action::Move { from, .. } => Some(from),
            Action::NoOp => None,
            Action::Gather { .. } | Action::Build { .. } => return command,
        };
        if let Some((at, building)) = self.site(view, busy) {
            command.action = Action::Build { at, building };
            self.last_build = Some(view.tick);
            self.builds += 1;
        }
        command
    }
}
