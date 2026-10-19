//! Foundry: one market for land, information, defense, movement, and construction.
//!
//! Every legal capture is priced from its dice-safe troop cost, transferred income,
//! denied rival income, terrain, new sight, and the post-move safety of both endpoints.
//! A topology gate removes exposed one-cell tendrils and attacks launched from critical
//! cut vertices. Relays, calm gathers, all four building types, and passing enter the
//! same score comparison as captures rather than running in a priority cascade.

use crate::{
    Action, Bot, Building, Hex, ManagementCommand, PlayerCommand, PlayerId, RulesConfig, Style,
    View, VisibleHex, MILLI,
};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct Cell {
    owner: Option<PlayerId>,
    troops: u32,
    wall_milli: u32,
    building: Option<Building>,
    income: u32,
}

const UNKNOWN: Cell = Cell {
    owner: None,
    troops: 10,
    wall_milli: MILLI,
    building: None,
    income: 1,
};

struct Map {
    radius: i32,
    width: i32,
    hexes: Vec<Hex>,
    passable: Vec<bool>,
    height: Vec<i8>,
    neighbors: Vec<[Option<usize>; 6]>,
    sight: Vec<Vec<usize>>,
}

impl Map {
    fn new(view: &View) -> Self {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let hexes: Vec<Hex> = (0..slots)
            .map(|i| Hex::new(i as i32 % width - radius, i as i32 / width - radius))
            .collect();
        let mut passable = vec![false; slots];
        let mut height = vec![0; slots];
        for i in 0..slots {
            if let Some(t) = view.public_terrain[i] {
                passable[i] = t.passable();
                height[i] = t.height;
            }
        }
        let index = |h: Hex| {
            (h.q.abs() <= radius && h.r.abs() <= radius && (h.q + h.r).abs() <= radius)
                .then(|| ((h.r + radius) * width + h.q + radius) as usize)
        };
        let neighbors = hexes.iter().map(|h| h.neighbors().map(index)).collect();
        let sight = hexes
            .iter()
            .map(|h| {
                let mut cells = vec![];
                for dq in -view.fog_radius..=view.fog_radius {
                    for dr in -view.fog_radius..=view.fog_radius {
                        let other = Hex::new(h.q + dq, h.r + dr);
                        if h.distance(other) > view.fog_radius {
                            continue;
                        }
                        if let Some(i) = index(other).filter(|&i| passable[i]) {
                            cells.push(i);
                        }
                    }
                }
                cells
            })
            .collect();
        Map {
            radius,
            width,
            hexes,
            passable,
            height,
            neighbors,
            sight,
        }
    }

    fn index(&self, h: Hex) -> Option<usize> {
        (h.q.abs() <= self.radius && h.r.abs() <= self.radius && (h.q + h.r).abs() <= self.radius)
            .then(|| ((h.r + self.radius) * self.width + h.q + self.radius) as usize)
    }
}

struct Knobs {
    sigma_x10: u64,
    land: i64,
    enemy: i64,
    income: i64,
    denial_pct: i64,
    village: i64,
    farm: i64,
    quarry: i64,
    captured_wall: i64,
    fog: i64,
    height: i64,
    hug: i64,
    exposure: i64,
    cost: i64,
    wealth: u64,
    guard: u64,
    hold_pct: u64,
    target_safety: i64,
    source_drop: i64,
    min_safety: i64,
    salient_own: usize,
    salient_enemy: usize,
    strict_until: u32,
    cut_safety: i64,
    attack_from: u32,
    early_attack_penalty: i64,
    break_from: u32,
    break_safety: i64,
    relay: i64,
    relay_distance: i64,
    gather: i64,
    gather_until: u32,
    build_from: u32,
    build_until: u32,
    build_gap: u32,
    build_depth: u32,
    build_income: i64,
    build_cost: i64,
    farm_future: i64,
    quarry_future: i64,
    wall_threat: i64,
    wall_choke: i64,
    build_cap: usize,
    sprint: u32,
    sprint_safety: i64,
    centre: i64,
    pass_score: i64,
}

impl Knobs {
    fn read(style: &Style) -> Self {
        let k = |name: &str, default: i64| style.params.knob(name, default).max(0);
        Knobs {
            sigma_x10: k("sigma_x10", 30) as u64,
            land: k("land", 1000),
            enemy: k("enemy", 300),
            income: k("income", 180),
            denial_pct: k("denial_pct", 100),
            village: k("village", 1100),
            farm: k("farm", 500),
            quarry: k("quarry", 700),
            captured_wall: k("captured_wall", 250),
            fog: k("fog", 30),
            height: k("height", 45),
            hug: k("hug", 180),
            exposure: k("exposure", 80),
            cost: k("cost", 12),
            wealth: k("wealth", 40) as u64,
            guard: k("guard", 20) as u64,
            hold_pct: k("hold_pct", 600) as u64,
            target_safety: k("target_safety", 700),
            source_drop: k("source_drop", 900),
            min_safety: k("min_safety", 300),
            salient_own: k("salient_own", 1) as usize,
            salient_enemy: k("salient_enemy", 2) as usize,
            strict_until: k("strict_until", 9000) as u32,
            cut_safety: k("cut_safety", 700),
            attack_from: k("attack_from", 0) as u32,
            early_attack_penalty: k("early_attack_penalty", 0),
            break_from: k("break_from", 0) as u32,
            break_safety: k("break_safety", 300),
            relay: k("relay", 900),
            relay_distance: k("relay_distance", 50),
            gather: k("gather", 500),
            gather_until: k("gather_until", 500) as u32,
            build_from: k("build_from", 150) as u32,
            build_until: k("build_until", 9000) as u32,
            build_gap: k("build_gap", 3) as u32,
            build_depth: k("build_depth", 1) as u32,
            build_income: k("build_income", 900),
            build_cost: k("build_cost", 5),
            farm_future: k("farm_future", 250),
            quarry_future: k("quarry_future", 180),
            wall_threat: k("wall_threat", 20),
            wall_choke: k("wall_choke", 350),
            build_cap: k("build_cap", 120) as usize,
            sprint: k("sprint", 700) as u32,
            sprint_safety: k("sprint_safety", 0),
            centre: k("centre", 0),
            pass_score: k("pass_score", 0),
        }
    }
}

struct Candidate {
    score: i64,
    action: Action,
    tie: (u8, usize, usize),
}

pub struct Foundry {
    name: String,
    rules: RulesConfig,
    k: Knobs,
    map: Option<Map>,
    cells: Vec<Cell>,
    visible: Vec<bool>,
    mine: Vec<bool>,
    win: u32,
    me: PlayerId,
    last_build: Option<u32>,
}

impl Foundry {
    pub fn new(style: Style) -> Self {
        Foundry {
            name: style.id.clone(),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            map: None,
            cells: vec![],
            visible: vec![],
            mine: vec![],
            win: RulesConfig::default().attacker_exchange_win_milli,
            me: 0,
            last_build: None,
        }
    }

    fn own(&self, i: usize) -> bool {
        self.mine[i]
    }

    fn enemy(&self, i: usize) -> bool {
        self.cells[i].owner.is_some_and(|owner| owner != self.me)
    }

    fn defense(&self, map: &Map, from: usize, to: usize, wall_milli: u32) -> u64 {
        let terrain = self
            .rules
            .height_multiplier_milli(map.height[from], map.height[to]) as u64;
        (terrain * wall_milli as u64 / 1000)
            .min(self.rules.maximum_total_defense_milli as u64)
            .max(1)
    }

    fn loss_stats(&self, defenders: u32, defense: u64) -> (u64, u64) {
        let exchanges_per_kill = (1_000_000 + defense / 2) / defense;
        let wins = (defenders as u64 * 1000).div_ceil(exchanges_per_kill.max(1));
        let p = self.win as u64;
        let q = 1000 - p;
        let mean_x1000 = wins * q * 1000 / p.max(1);
        let variance_x1m = wins * q * 1_000_000_000 / (p * p).max(1);
        (mean_x1000, integer_sqrt(variance_x1m))
    }

    fn kills(&self, attackers: u64, defense: u64) -> u64 {
        attackers * self.win as u64 * 1000 / ((1000 - self.win as u64).max(1) * defense.max(1))
    }

    fn border_need(&self, map: &Map, at: usize, ignore: Option<usize>, wall: u32) -> Option<u64> {
        let mut worst = None;
        for n in map.neighbors[at].into_iter().flatten() {
            if Some(n) == ignore || !self.enemy(n) {
                continue;
            }
            let available = self.cells[n].troops.saturating_sub(1) as u64;
            let defense = self.defense(map, n, at, wall);
            let threat = self.kills(available, defense);
            worst = Some(worst.map_or(threat, |old: u64| old.max(threat)));
        }
        worst.map(|threat| self.k.guard + threat * self.k.hold_pct / 100)
    }

    fn safety(troops: u64, need: Option<u64>) -> i64 {
        match need {
            None => 1000,
            Some(need) => (troops * 1000 / need.max(1)).min(1000) as i64,
        }
    }

    fn articulation_points(&self, map: &Map) -> Vec<bool> {
        let slots = self.mine.len();
        let mut discovery = vec![0u32; slots];
        let mut low = vec![0u32; slots];
        let mut cut = vec![false; slots];
        let mut clock = 0;
        for root in 0..slots {
            if self.own(root) && discovery[root] == 0 {
                articulation_dfs(
                    root,
                    usize::MAX,
                    map,
                    &self.mine,
                    &mut discovery,
                    &mut low,
                    &mut cut,
                    &mut clock,
                );
            }
        }
        cut
    }

    fn fog_gain(&self, map: &Map, target: usize) -> usize {
        map.sight[target]
            .iter()
            .filter(|&&i| !self.visible[i])
            .count()
    }

    fn building_value(&self, cell: Cell) -> i64 {
        match cell.building {
            Some(Building::Village) => self.k.village,
            Some(Building::Farm) => self.k.farm,
            Some(Building::Quarry) => self.k.quarry,
            Some(Building::Wall) => self.k.captured_wall,
            None => 0,
        }
    }

    fn capture_candidates(
        &self,
        map: &Map,
        own: &[usize],
        cut: &[bool],
        tick: u32,
        total_troops: u64,
        out: &mut Vec<Candidate>,
    ) {
        let sprinting = tick.saturating_add(self.k.sprint) >= self.rules.max_ticks;
        let cost_scale = if self.k.wealth == 0 {
            1000
        } else {
            let average = total_troops / own.len().max(1) as u64;
            1000 * self.k.wealth / (self.k.wealth + average)
        } as i64;
        for &from in own {
            let source = self.cells[from];
            if source.troops < 2 {
                continue;
            }
            let source_need_before = self.border_need(map, from, None, source.wall_milli);
            let source_before = Self::safety(source.troops as u64, source_need_before);
            for to in map.neighbors[from].into_iter().flatten() {
                if !map.passable[to] || self.own(to) || !self.visible[to] {
                    continue;
                }
                let target = self.cells[to];
                let defense = self.defense(map, from, to, target.wall_milli);
                let (mean_x1000, deviation_x1000) = self.loss_stats(target.troops, defense);
                let sure_loss =
                    (mean_x1000 + deviation_x1000 * self.k.sigma_x10 / 10).div_ceil(1000);
                let expected_loss = mean_x1000.div_ceil(1000);
                let attack_min = sure_loss + 1;
                let available = source.troops as u64 - 1;
                if attack_min > available {
                    continue;
                }

                let own_neighbors = map.neighbors[to]
                    .into_iter()
                    .flatten()
                    .filter(|&n| self.own(n))
                    .count();
                let enemy_neighbors = map.neighbors[to]
                    .into_iter()
                    .flatten()
                    .filter(|&n| self.enemy(n))
                    .count();
                let open_neighbors = map.neighbors[to]
                    .into_iter()
                    .flatten()
                    .filter(|&n| map.passable[n] && !self.own(n))
                    .count();
                let strict = !sprinting && tick < self.k.strict_until;
                if strict
                    && own_neighbors <= self.k.salient_own
                    && enemy_neighbors >= self.k.salient_enemy
                {
                    continue;
                }
                if strict && own_neighbors == 1 && open_neighbors >= 3 && enemy_neighbors > 0 {
                    continue;
                }

                let target_wall = if target.building == Some(Building::Wall) {
                    self.rules.wall_multiplier_milli(0)
                } else {
                    MILLI
                };
                let target_need = self.border_need(map, to, Some(from), target_wall);
                let source_need_after = self.border_need(map, from, Some(to), source.wall_milli);
                let desired_send = expected_loss + target_need.unwrap_or(0);
                let source_keep = source_need_after.unwrap_or(1).max(1);
                let source_limited = (source.troops as u64).saturating_sub(source_keep);
                let send = desired_send
                    .max(attack_min)
                    .min(available)
                    .min(source_limited.max(attack_min));
                let target_left = send.saturating_sub(expected_loss).max(1);
                let source_left = source.troops as u64 - send;
                let target_safety = Self::safety(target_left, target_need);
                let source_safety = Self::safety(source_left, source_need_after);
                let minimum = if sprinting {
                    self.k.sprint_safety
                } else if self.k.break_from > 0 && tick >= self.k.break_from {
                    self.k.break_safety
                } else {
                    self.k.min_safety
                };
                if target_safety < minimum || source_safety < minimum {
                    continue;
                }
                if strict && cut[from] && source_safety < self.k.cut_safety {
                    continue;
                }

                let enemy = target.owner.is_some();
                let income = target.income.max(1) as i64;
                let denied = if enemy {
                    income * self.k.denial_pct / 100
                } else {
                    0
                };
                let mut score = self.k.land
                    + self.k.income * (income + denied)
                    + self.building_value(target)
                    + self.k.fog * self.fog_gain(map, to) as i64
                    + self.k.height * map.height[to] as i64
                    + self.k.hug * own_neighbors as i64
                    - self.k.exposure * open_neighbors as i64
                    - self.k.cost * expected_loss as i64 * cost_scale / 1000
                    + self.k.target_safety * target_safety / 1000
                    - self.k.source_drop * (source_before - source_safety).max(0) / 1000;
                if enemy {
                    score += self.k.enemy;
                }
                if self.k.centre > 0 {
                    score -= self.k.centre * map.hexes[to].distance(Hex::new(0, 0)) as i64;
                }
                if sprinting {
                    score += self.k.land;
                }
                if tick < self.k.attack_from {
                    score -= self.k.early_attack_penalty;
                }
                out.push(Candidate {
                    score,
                    action: Action::Move {
                        from: map.hexes[from],
                        to: map.hexes[to],
                        troops: send as u32,
                    },
                    tie: (4, to, from),
                });
            }
        }
    }

    fn frontier_data(&self, map: &Map, own: &[usize]) -> (Vec<u32>, Vec<usize>, Vec<usize>) {
        let slots = self.cells.len();
        let mut distance = vec![u32::MAX; slots];
        let mut next = vec![usize::MAX; slots];
        let mut target = vec![usize::MAX; slots];
        let mut queue = VecDeque::new();
        for &i in own {
            let need = self.border_need(map, i, None, self.cells[i].wall_milli);
            let short = need.is_some_and(|n| self.cells[i].troops as u64 <= n);
            let blocked_value = map.neighbors[i].into_iter().flatten().any(|n| {
                map.passable[n]
                    && !self.own(n)
                    && self.visible[n]
                    && (self.cells[n].building.is_some() || self.cells[n].income > 1)
            });
            if short || blocked_value {
                distance[i] = 0;
                target[i] = i;
                queue.push_back(i);
            }
        }
        if queue.is_empty() {
            for &i in own {
                if map.neighbors[i]
                    .into_iter()
                    .flatten()
                    .any(|n| map.passable[n] && !self.own(n))
                {
                    distance[i] = 0;
                    target[i] = i;
                    queue.push_back(i);
                }
            }
        }
        while let Some(at) = queue.pop_front() {
            for n in map.neighbors[at].into_iter().flatten() {
                if self.own(n) && distance[n] == u32::MAX {
                    distance[n] = distance[at] + 1;
                    next[n] = at;
                    target[n] = target[at];
                    queue.push_back(n);
                }
            }
        }
        (distance, next, target)
    }

    fn movement_candidates(&self, map: &Map, own: &[usize], tick: u32, out: &mut Vec<Candidate>) {
        let (distance, next, target) = self.frontier_data(map, own);
        let contact = own.iter().any(|&i| {
            map.neighbors[i]
                .into_iter()
                .flatten()
                .any(|n| self.enemy(n))
        });
        for &from in own {
            if distance[from] == 0 || distance[from] == u32::MAX {
                continue;
            }
            let keep = self
                .border_need(map, from, None, self.cells[from].wall_milli)
                .unwrap_or(1)
                .max(1);
            let spare = (self.cells[from].troops as u64).saturating_sub(keep);
            if spare == 0 {
                continue;
            }
            let goal = target[from];
            let need = self.border_need(map, goal, None, self.cells[goal].wall_milli);
            let before = Self::safety(self.cells[goal].troops as u64, need);
            let after = Self::safety(self.cells[goal].troops as u64 + spare, need);
            let gain = (after - before).max(0);
            let score = self.k.relay * gain / 1000 + (spare.min(500) as i64)
                - self.k.relay_distance * distance[from] as i64;
            out.push(Candidate {
                score,
                action: Action::Move {
                    from: map.hexes[from],
                    to: map.hexes[next[from]],
                    troops: spare as u32,
                },
                tie: (2, goal, from),
            });
        }
        if !contact && tick < self.k.gather_until && own.len() > 2 {
            let mut best_goal = None;
            for &i in own {
                if distance[i] != 0 {
                    continue;
                }
                let delivered: u64 = own
                    .iter()
                    .filter(|&&j| distance[j] != u32::MAX)
                    .map(|&j| self.cells[j].troops.saturating_sub(1) as u64)
                    .sum();
                let score = self.k.gather + delivered.min(1000) as i64;
                if best_goal.is_none_or(|(old, _)| score > old) {
                    best_goal = Some((score, i));
                }
            }
            if let Some((score, at)) = best_goal {
                out.push(Candidate {
                    score,
                    action: Action::Gather {
                        target: map.hexes[at],
                    },
                    tie: (1, at, at),
                });
            }
        }
    }

    fn depth_from_border(&self, map: &Map) -> Vec<u32> {
        let mut depth = vec![u32::MAX; self.cells.len()];
        let mut queue = VecDeque::new();
        for i in 0..self.cells.len() {
            if map.passable[i] && !self.own(i) {
                depth[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(at) = queue.pop_front() {
            for n in map.neighbors[at].into_iter().flatten() {
                if self.own(n) && depth[n] == u32::MAX {
                    depth[n] = depth[at] + 1;
                    queue.push_back(n);
                }
            }
        }
        depth
    }

    fn build_candidates(
        &self,
        view: &View,
        map: &Map,
        own: &[usize],
        tick: u32,
        out: &mut Vec<Candidate>,
    ) {
        let building_count = own
            .iter()
            .filter(|&&i| self.cells[i].building.is_some())
            .count();
        if building_count >= self.k.build_cap
            || tick < self.k.build_from
            || (self.k.build_until > 0 && tick >= self.k.build_until)
            || self
                .last_build
                .is_some_and(|last| tick < last + self.k.build_gap)
        {
            return;
        }
        let depth = self.depth_from_border(map);
        for &at in own {
            let cell = self.cells[at];
            if cell.building.is_some() {
                continue;
            }
            let adjacent_quarry = map.neighbors[at]
                .into_iter()
                .flatten()
                .any(|n| self.own(n) && self.cells[n].building == Some(Building::Quarry));
            let farms = map.neighbors[at]
                .into_iter()
                .flatten()
                .filter(|&n| self.own(n) && self.cells[n].building == Some(Building::Farm))
                .count() as i64;
            let villages = map.neighbors[at]
                .into_iter()
                .flatten()
                .filter(|&n| self.own(n) && self.cells[n].building == Some(Building::Village))
                .count() as i64;
            let empties = map.neighbors[at]
                .into_iter()
                .flatten()
                .filter(|&n| self.own(n) && self.cells[n].building.is_none())
                .count() as i64;
            let enemy_threat = self
                .border_need(map, at, None, cell.wall_milli)
                .unwrap_or(0) as i64;
            for kind in Building::ALL {
                if !self.rules.terrain_allowed(kind, map.height[at]) {
                    continue;
                }
                let interior = depth[at] >= self.k.build_depth;
                if kind != Building::Wall && !interior {
                    continue;
                }
                if kind == Building::Wall && enemy_threat == 0 {
                    continue;
                }
                if kind == Building::Quarry {
                    let too_close = own.iter().any(|&i| {
                        self.cells[i].building == Some(Building::Quarry)
                            && map.hexes[i].distance(map.hexes[at])
                                < self.rules.quarry.min_friendly_distance
                    });
                    if too_close {
                        continue;
                    }
                }
                let cost = self.rules.build_cost(kind, adjacent_quarry);
                if view.own_economy.resource < cost.resource || cell.troops <= cost.troops {
                    continue;
                }
                let (income_gain, position_gain) = match kind {
                    Building::Village => {
                        let quarry =
                            i64::from(adjacent_quarry) * self.rules.quarry.village_bonus as i64;
                        (
                            self.rules.village.bonus as i64
                                + farms * self.rules.village.farm_bonus as i64
                                + quarry,
                            farms * self.k.farm_future,
                        )
                    }
                    Building::Farm => (
                        villages * self.rules.village.farm_bonus as i64,
                        empties * self.k.farm_future,
                    ),
                    Building::Quarry => (
                        villages * self.rules.quarry.village_bonus as i64,
                        empties * self.k.quarry_future,
                    ),
                    Building::Wall => {
                        let choke = (6usize.saturating_sub(
                            map.neighbors[at]
                                .into_iter()
                                .flatten()
                                .filter(|&n| map.passable[n])
                                .count(),
                        )) as i64;
                        (
                            0,
                            enemy_threat * self.k.wall_threat + choke * self.k.wall_choke,
                        )
                    }
                };
                let score = income_gain * self.k.build_income + position_gain
                    - cost.troops as i64 * self.k.build_cost;
                out.push(Candidate {
                    score,
                    action: Action::Build {
                        at: map.hexes[at],
                        building: kind,
                    },
                    tie: (3, at, kind_order(kind)),
                });
            }
        }
    }

    fn management(&self, view: &View) -> Vec<ManagementCommand> {
        let mine: Vec<&VisibleHex> = view
            .visible_hexes
            .iter()
            .filter(|c| c.owner == Some(view.you))
            .collect();
        let mut commands = vec![];
        if !self.rules.farm.adjacent_boost {
            'farm: for farm in mine.iter().filter(|c| {
                c.building.as_ref().is_some_and(|b| {
                    b.kind == Building::Farm
                        && b.link_cooldown == 0
                        && b.links.len() < self.rules.farm.sockets as usize
                })
            }) {
                if let Some(village) = mine.iter().find(|v| {
                    v.hex.distance(farm.hex) == 1
                        && v.building
                            .as_ref()
                            .is_some_and(|b| b.kind == Building::Village && b.links.is_empty())
                }) {
                    commands.push(ManagementCommand::SetFarmLink {
                        farm: farm.hex,
                        village: village.hex,
                        enabled: true,
                    });
                    break 'farm;
                }
            }
        }
        let max_links = self.rules.wall.max_links as usize;
        'walls: for a in mine.iter().filter(|c| {
            c.building.as_ref().is_some_and(|b| {
                b.kind == Building::Wall && b.link_cooldown == 0 && b.links.len() < max_links
            })
        }) {
            for b in mine.iter().filter(|c| {
                c.hex != a.hex
                    && c.building.as_ref().is_some_and(|building| {
                        building.kind == Building::Wall
                            && building.link_cooldown == 0
                            && building.links.len() < max_links
                    })
            }) {
                let already_linked = a
                    .building
                    .as_ref()
                    .is_some_and(|building| building.links.contains(&b.hex));
                if a.hex.distance(b.hex) == 1 && !already_linked {
                    commands.push(ManagementCommand::SetWallLink {
                        wall_a: a.hex,
                        wall_b: b.hex,
                        enabled: true,
                    });
                    break 'walls;
                }
            }
        }
        commands
    }
}

impl Bot for Foundry {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if let Some(rules) = &view.rules {
            self.rules = rules.clone();
        }
        self.win = self.rules.attacker_win_milli(view.tick);
        if self
            .map
            .as_ref()
            .is_none_or(|map| map.radius != view.radius)
        {
            let map = Map::new(view);
            self.cells = vec![UNKNOWN; map.hexes.len()];
            self.visible = vec![false; map.hexes.len()];
            self.mine = vec![false; map.hexes.len()];
            self.map = Some(map);
        }
        let map = self.map.take().expect("map initialized");
        self.me = view.you;
        self.visible.fill(false);
        self.mine.fill(false);
        for cell in &view.visible_hexes {
            let Some(i) = map.index(cell.hex) else {
                continue;
            };
            self.visible[i] = true;
            self.mine[i] = cell.owner == Some(view.you);
            self.cells[i] = Cell {
                owner: cell.owner,
                troops: cell.troops,
                wall_milli: cell.wall_milli,
                building: cell.building.as_ref().map(|b| b.kind),
                income: cell.income,
            };
        }
        for i in 0..self.cells.len() {
            if self.cells[i].owner == Some(self.me) && !self.mine[i] {
                self.cells[i] = UNKNOWN;
            }
        }
        let own: Vec<usize> = (0..self.cells.len()).filter(|&i| self.own(i)).collect();
        let total_troops = own.iter().map(|&i| self.cells[i].troops as u64).sum();
        let cut = self.articulation_points(&map);
        let mut candidates = vec![Candidate {
            score: self.k.pass_score,
            action: Action::NoOp,
            tie: (0, 0, 0),
        }];
        self.capture_candidates(&map, &own, &cut, view.tick, total_troops, &mut candidates);
        self.movement_candidates(&map, &own, view.tick, &mut candidates);
        self.build_candidates(view, &map, &own, view.tick, &mut candidates);
        let best = candidates
            .into_iter()
            .max_by_key(|candidate| (candidate.score, candidate.tie))
            .expect("pass candidate exists");
        if matches!(best.action, Action::Build { .. }) {
            self.last_build = Some(view.tick);
        }
        let management = self.management(view);
        self.map = Some(map);
        PlayerCommand {
            management,
            action: best.action,
        }
    }
}

fn articulation_dfs(
    at: usize,
    parent: usize,
    map: &Map,
    own: &[bool],
    discovery: &mut [u32],
    low: &mut [u32],
    cut: &mut [bool],
    clock: &mut u32,
) {
    *clock += 1;
    discovery[at] = *clock;
    low[at] = *clock;
    let mut children = 0;
    for next in map.neighbors[at].into_iter().flatten() {
        if !own[next] {
            continue;
        }
        if discovery[next] == 0 {
            children += 1;
            articulation_dfs(next, at, map, own, discovery, low, cut, clock);
            low[at] = low[at].min(low[next]);
            if parent != usize::MAX && low[next] >= discovery[at] {
                cut[at] = true;
            }
        } else if next != parent {
            low[at] = low[at].min(discovery[next]);
        }
    }
    if parent == usize::MAX && children > 1 {
        cut[at] = true;
    }
}

fn integer_sqrt(value: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut root = (value as f64).sqrt() as u64;
    while root.saturating_mul(root) > value {
        root -= 1;
    }
    while (root + 1).saturating_mul(root + 1) <= value {
        root += 1;
    }
    root
}

fn kind_order(kind: Building) -> usize {
    match kind {
        Building::Farm => 0,
        Building::Village => 1,
        Building::Quarry => 2,
        Building::Wall => 3,
    }
}
