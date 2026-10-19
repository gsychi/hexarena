//! Grazer: Relay's proven capture/relay executor with a geometry-aware troop economy.
//!
//! Villages occupy one colour of a hex three-colouring and Farms the other two, so a
//! mature Village can touch six Farms while each Farm boosts up to three Villages. An
//! optional Quarry plan interleaves hill Quarries before the lattice for discounts and
//! the live-rule Village adjacency bonus. Construction is restricted by owned-path depth;
//! every non-build tick is delegated unchanged to Relay.

use super::{predator::Predator, relay::Relay, ridgeline::Ridgeline};
use crate::{Bot, Style};
use hexarena_protocol::{
    Action, Building, Hex, ManagementCommand, PlayerCommand, PlayerId, RulesConfig, View,
    VisibleHex,
};
use std::collections::{BTreeMap, VecDeque};

fn visible(view: &View, hex: Hex) -> Option<&VisibleHex> {
    view.visible_hexes.iter().find(|cell| cell.hex == hex)
}

fn terrain_height(view: &View, hex: Hex) -> i8 {
    let width = 2 * view.radius + 1;
    if hex.q.abs() > view.radius || hex.r.abs() > view.radius || (hex.q + hex.r).abs() > view.radius
    {
        return 0;
    }
    let index = ((hex.r + view.radius) * width + hex.q + view.radius) as usize;
    view.public_terrain
        .get(index)
        .copied()
        .flatten()
        .map_or(0, |terrain| terrain.height)
}

struct Knobs {
    executor_mode: i64,
    mode: i64,
    start: u32,
    stop: u32,
    every: u32,
    cap: usize,
    quarry_cap: usize,
    land_min: usize,
    depth: u32,
    keep: u32,
    wall_start: u32,
    wall_stop: u32,
    wall_every: u32,
    wall_cap: usize,
    wall_keep: u32,
    wall_contact_only: bool,
    finisher_start: u32,
    finisher_alive_max: usize,
    finisher_land_min: usize,
    finisher_margin_pct: u64,
    finisher_forward_pct: u64,
    finisher_keep: u64,
    finisher_garrison: u64,
}

impl Knobs {
    fn read(style: &Style) -> Self {
        let k = |name: &str, default: i64| style.params.knob(name, default).max(0);
        Self {
            executor_mode: k("executor_mode", 1),
            mode: k("economy_mode", 1),
            start: k("economy_start", 500) as u32,
            stop: k("economy_stop", 5000) as u32,
            every: k("economy_every", 6).max(1) as u32,
            cap: k("economy_cap", 120) as usize,
            quarry_cap: k("economy_quarry_cap", 4) as usize,
            land_min: k("economy_land_min", 45) as usize,
            depth: k("economy_depth", 2) as u32,
            keep: k("economy_keep", 20) as u32,
            wall_start: k("wall_start", 1000) as u32,
            wall_stop: k("wall_stop", 8000) as u32,
            wall_every: k("wall_every", 4).max(1) as u32,
            wall_cap: k("wall_cap", 0) as usize,
            wall_keep: k("wall_keep", 30) as u32,
            wall_contact_only: k("wall_contact_only", 1) != 0,
            finisher_start: k("finisher_start", 4500) as u32,
            finisher_alive_max: k("finisher_alive_max", 0) as usize,
            finisher_land_min: k("finisher_land_min", 100) as usize,
            finisher_margin_pct: k("finisher_margin_pct", 140) as u64,
            finisher_forward_pct: k("finisher_forward_pct", 70).min(100) as u64,
            finisher_keep: k("finisher_keep", 30) as u64,
            finisher_garrison: k("finisher_garrison", 20) as u64,
        }
    }
}

pub struct Grazer {
    name: String,
    inner: Box<dyn Bot>,
    rules: RulesConfig,
    k: Knobs,
    last_build: Option<u32>,
    builds: usize,
    last_wall: Option<u32>,
    walls: usize,
    hunt_target: Option<PlayerId>,
}

impl Grazer {
    pub fn new(style: Style) -> Self {
        let k = Knobs::read(&style);
        let inner: Box<dyn Bot> = match k.executor_mode {
            2 => Box::new(Ridgeline::new(style.clone())),
            3 => Box::new(Predator::new(style.clone())),
            _ => Box::new(Relay::new(style.clone())),
        };
        Self {
            name: style.id.clone(),
            inner,
            rules: RulesConfig::default(),
            k,
            last_build: None,
            builds: 0,
            last_wall: None,
            walls: 0,
            hunt_target: None,
        }
    }

    fn finisher(&mut self, view: &View) -> Option<Action> {
        let alive = view
            .public_players
            .iter()
            .filter(|player| player.alive)
            .count();
        let own_land = view
            .visible_hexes
            .iter()
            .filter(|cell| cell.owner == Some(view.you))
            .count();
        if self.k.finisher_alive_max == 0
            || view.tick < self.k.finisher_start
            || alive > self.k.finisher_alive_max
            || own_land < self.k.finisher_land_min
        {
            return None;
        }
        if self.hunt_target.is_some_and(|target| {
            !view
                .public_players
                .iter()
                .any(|player| player.id == target && player.alive)
        }) {
            self.hunt_target = None;
        }
        let mut visible_land = BTreeMap::<PlayerId, usize>::new();
        let mut contacts = BTreeMap::<PlayerId, usize>::new();
        for cell in &view.visible_hexes {
            let Some(owner) = cell.owner.filter(|owner| *owner != view.you) else {
                continue;
            };
            *visible_land.entry(owner).or_default() += 1;
        }
        for cell in &view.visible_hexes {
            if cell.owner != Some(view.you) {
                continue;
            }
            for neighbor in cell.hex.neighbors() {
                if let Some(owner) =
                    visible(view, neighbor).and_then(|other| other.owner.filter(|p| *p != view.you))
                {
                    *contacts.entry(owner).or_default() += 1;
                }
            }
        }
        if self
            .hunt_target
            .is_none_or(|target| !contacts.contains_key(&target))
        {
            self.hunt_target = contacts.keys().copied().min_by_key(|target| {
                (
                    visible_land.get(target).copied().unwrap_or(usize::MAX),
                    std::cmp::Reverse(contacts.get(target).copied().unwrap_or(0)),
                    *target,
                )
            });
        }
        let target = self.hunt_target?;
        let win = self.rules.attacker_win_milli(view.tick) as u64;
        let mut best: Option<(u64, Action)> = None;
        for source in &view.visible_hexes {
            if source.owner != Some(view.you) {
                continue;
            }
            for destination_hex in source.hex.neighbors() {
                let Some(destination) =
                    visible(view, destination_hex).filter(|cell| cell.owner == Some(target))
                else {
                    continue;
                };
                let height = self.rules.height_multiplier_milli(
                    terrain_height(view, source.hex),
                    terrain_height(view, destination_hex),
                ) as u64;
                let defense = (height * destination.wall_milli as u64 / 1000)
                    .min(self.rules.maximum_total_defense_milli as u64);
                let loss = destination.troops as u64 * defense * (1000 - win) / win / 1000;
                let touches_third_party = source.hex.neighbors().into_iter().any(|neighbor| {
                    visible(view, neighbor).is_some_and(|cell| {
                        cell.owner
                            .is_some_and(|owner| owner != view.you && owner != target)
                    })
                });
                let keep = if touches_third_party {
                    self.k.finisher_keep
                } else {
                    1
                };
                let minimum =
                    loss * self.k.finisher_margin_pct / 100 + self.k.finisher_garrison + 1;
                let available = source.troops as u64;
                if available <= keep + minimum {
                    continue;
                }
                let surplus = available - keep - minimum;
                let send = minimum + surplus * self.k.finisher_forward_pct / 100;
                let own_neighbors = destination_hex
                    .neighbors()
                    .into_iter()
                    .filter(|neighbor| {
                        visible(view, *neighbor).is_some_and(|cell| cell.owner == Some(view.you))
                    })
                    .count() as u64;
                let score = 10_000
                    + own_neighbors * 500
                    + destination
                        .building
                        .as_ref()
                        .map_or(0, |building| match building.kind {
                            Building::Village => 1_000,
                            Building::Farm => 400,
                            Building::Quarry => 700,
                            Building::Wall => 200,
                        })
                    + send.saturating_sub(loss);
                let action = Action::Move {
                    from: source.hex,
                    to: destination_hex,
                    troops: send.min(u32::MAX as u64) as u32,
                };
                if best.as_ref().is_none_or(|current| score > current.0) {
                    best = Some((score, action));
                }
            }
        }
        best.map(|(_, action)| action)
    }

    fn wall_site(&self, view: &View, busy: Option<Hex>) -> Option<Hex> {
        if self.walls >= self.k.wall_cap
            || view.tick < self.k.wall_start
            || view.tick >= self.k.wall_stop
            || self
                .last_wall
                .is_some_and(|tick| view.tick < tick + self.k.wall_every)
        {
            return None;
        }
        let mut best: Option<(u64, Hex)> = None;
        for cell in &view.visible_hexes {
            if cell.owner != Some(view.you)
                || cell.building.is_some()
                || busy == Some(cell.hex)
                || !self
                    .rules
                    .terrain_allowed(Building::Wall, terrain_height(view, cell.hex))
            {
                continue;
            }
            let discounted = cell.hex.neighbors().into_iter().any(|neighbor| {
                visible(view, neighbor).is_some_and(|other| {
                    other.owner == Some(view.you)
                        && other
                            .building
                            .as_ref()
                            .is_some_and(|building| building.kind == Building::Quarry)
                })
            });
            let cost = self.rules.build_cost(Building::Wall, discounted);
            if cell.troops <= cost.troops + self.k.wall_keep {
                continue;
            }
            let mut enemy_contacts = 0u64;
            let mut largest_enemy = 0u64;
            let mut foreign_contacts = 0u64;
            let mut adjacent_walls = 0u64;
            let mut own_neighbors = 0u64;
            for neighbor in cell.hex.neighbors() {
                let Some(other) = visible(view, neighbor) else {
                    continue;
                };
                if other.owner == Some(view.you) {
                    own_neighbors += 1;
                    if other
                        .building
                        .as_ref()
                        .is_some_and(|building| building.kind == Building::Wall)
                    {
                        adjacent_walls += 1;
                    }
                } else {
                    foreign_contacts += 1;
                    if other.owner.is_some() {
                        enemy_contacts += 1;
                        largest_enemy = largest_enemy.max(other.troops as u64);
                    }
                }
            }
            if foreign_contacts == 0 || (self.k.wall_contact_only && enemy_contacts == 0) {
                continue;
            }
            let score = enemy_contacts * 2_000
                + largest_enemy * 10
                + adjacent_walls * 700
                + own_neighbors * 30
                + cell.troops as u64;
            if best.is_none_or(|current| score > current.0) {
                best = Some((score, cell.hex));
            }
        }
        best.map(|(_, hex)| hex)
    }

    fn wall_link(&self, view: &View) -> Option<ManagementCommand> {
        for cell in &view.visible_hexes {
            let Some(wall) = cell.building.as_ref().filter(|building| {
                cell.owner == Some(view.you)
                    && building.kind == Building::Wall
                    && building.link_cooldown == 0
                    && building.links.len() < self.rules.wall.max_links as usize
            }) else {
                continue;
            };
            for neighbor in cell.hex.neighbors() {
                let Some(other) = visible(view, neighbor) else {
                    continue;
                };
                let Some(_other_wall) = other.building.as_ref().filter(|building| {
                    other.owner == Some(view.you)
                        && building.kind == Building::Wall
                        && building.link_cooldown == 0
                        && building.links.len() < self.rules.wall.max_links as usize
                }) else {
                    continue;
                };
                if !wall.links.contains(&neighbor) {
                    return Some(ManagementCommand::SetWallLink {
                        wall_a: cell.hex,
                        wall_b: neighbor,
                        enabled: true,
                    });
                }
            }
        }
        None
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
        if let Some(action) = self.finisher(view) {
            command.action = action;
        } else if let Some(at) = self.wall_site(view, busy) {
            command.action = Action::Build {
                at,
                building: Building::Wall,
            };
            self.last_wall = Some(view.tick);
            self.walls += 1;
        } else if let Some((at, building)) = self.site(view, busy) {
            command.action = Action::Build { at, building };
            self.last_build = Some(view.tick);
            self.builds += 1;
        }
        if let Some(link) = self.wall_link(view) {
            command.management.push(link);
        }
        command
    }
}
