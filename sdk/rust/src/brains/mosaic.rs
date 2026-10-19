//! Mosaic: adaptive free-for-all strategies built around a shared tactical executor.
//!
//! The brain treats expansion as a sequence of border auctions. Each affordable target
//! bids with its terrain, the open space behind it, the shape it gives our border, and
//! the diplomatic consequences of taking it. A doctrine knob changes those consequences:
//! cartographers seek unexplored basins, vultures finish small rivals, stewards invest in
//! linked Farms and Villages, and avengers remember who captured their land.

use crate::{Bot, PlayerCommand, Style};
use hexarena_protocol::{
    Action, Building, Event, Hex, ManagementCommand, PlayerId, RulesConfig, Terrain, View,
    VisibleBuilding, MILLI,
};
use std::collections::VecDeque;

#[derive(Clone)]
struct Cell {
    owner: Option<PlayerId>,
    troops: u32,
    wall: u32,
    building: Option<VisibleBuilding>,
}

struct Knobs {
    doctrine: i64,
    margin_pct: u64,
    endgame_margin_pct: u64,
    margin_abs: u64,
    opening_land: usize,
    guard: u64,
    hold_pct: u64,
    forward_pct: u64,
    neutral: i64,
    enemy: i64,
    space: i64,
    hug: i64,
    high: i64,
    cost: i64,
    leader: i64,
    prey: i64,
    revenge: i64,
    counter_bonus: i64,
    finish_tick: u32,
    finish_alive: usize,
    finish_ratio_pct: i64,
    finish_bonus: i64,
    finish_target_mode: i64,
    hunt_value: i64,
    lookahead_value: i64,
    enclose_value: i64,
    relay_floor: u64,
    relay_patience: i64,
    allow_gather: bool,
    adaptive_period: u32,
    momentum_weight: i64,
    peace_until: u32,
    build_mode: i64,
    build_start: u32,
    build_stop: u32,
    build_every: u32,
    build_cap: usize,
    quarry_cap: usize,
    aggression_tick: u32,
    logistics_min: u32,
    wave_cycle: u32,
    wave_charge: u32,
}

impl Knobs {
    fn read(style: &Style) -> Self {
        let k = |name: &str, default: i64| style.params.knob(name, default);
        Self {
            doctrine: k("doctrine", 0),
            margin_pct: k("margin_pct", 145).max(100) as u64,
            endgame_margin_pct: k("endgame_margin_pct", 118).max(100) as u64,
            margin_abs: k("margin_abs", 5).max(1) as u64,
            opening_land: k("opening_land", 5).max(0) as usize,
            guard: k("guard", 12).max(0) as u64,
            hold_pct: k("hold_pct", 25).max(0) as u64,
            forward_pct: k("forward_pct", 50).clamp(0, 100) as u64,
            neutral: k("neutral", 120),
            enemy: k("enemy", 35),
            space: k("space", 9),
            hug: k("hug", 15),
            high: k("high", 8),
            cost: k("cost", 35),
            leader: k("leader", 80),
            prey: k("prey", 90),
            revenge: k("revenge", 18),
            counter_bonus: k("counter_bonus", 0),
            finish_tick: k("finish_tick", 1800).max(0) as u32,
            finish_alive: k("finish_alive", 4).max(1) as usize,
            finish_ratio_pct: k("finish_ratio_pct", 110).max(100),
            finish_bonus: k("finish_bonus", 0),
            finish_target_mode: k("finish_target_mode", 0),
            hunt_value: k("hunt_value", 0),
            lookahead_value: k("lookahead_value", 0),
            enclose_value: k("enclose_value", 0),
            relay_floor: k("relay_floor", 1).max(1) as u64,
            relay_patience: k("relay_patience", 1000).max(0),
            allow_gather: k("allow_gather", 1) != 0,
            adaptive_period: k("adaptive_period", 240).max(30) as u32,
            momentum_weight: k("momentum_weight", 0),
            peace_until: k("peace_until", 500).max(0) as u32,
            build_mode: k("build_mode", 0),
            build_start: k("build_start", 0).max(0) as u32,
            build_stop: k("build_stop", 10_000).max(0) as u32,
            build_every: k("build_every", 11).max(1) as u32,
            build_cap: k("build_cap", 8).max(0) as usize,
            quarry_cap: k("quarry_cap", 2).max(0) as usize,
            aggression_tick: k("aggression_tick", 3000).max(0) as u32,
            logistics_min: k("logistics_min", 80).max(2) as u32,
            wave_cycle: k("wave_cycle", 0).max(0) as u32,
            wave_charge: k("wave_charge", 0).max(0) as u32,
        }
    }
}

#[derive(Clone, Copy)]
struct Bid {
    score: i64,
    from: usize,
    to: usize,
    send: u32,
    short: u64,
}

pub struct Mosaic {
    name: String,
    k: Knobs,
    rules: RulesConfig,
    radius: i32,
    width: usize,
    hexes: Vec<Hex>,
    terrain: Vec<Option<Terrain>>,
    neighbors: Vec<Vec<usize>>,
    seen: Vec<Option<Cell>>,
    own: Vec<bool>,
    exposed_until: Vec<u32>,
    hostility: Vec<i64>,
    last_decay: u32,
    last_build: u32,
    build_actions: usize,
    active_policy: usize,
    policy_gain: [i64; 3],
    policy_trials: [u32; 3],
    policy_started: u32,
    policy_land: i64,
    rival_momentum: Vec<i64>,
    last_land: Vec<i64>,
    last_model_tick: u32,
    tick: u32,
}

impl Mosaic {
    pub fn new(style: Style) -> Self {
        Self {
            name: style.id.clone(),
            k: Knobs::read(&style),
            rules: RulesConfig::default(),
            radius: -1,
            width: 0,
            hexes: vec![],
            terrain: vec![],
            neighbors: vec![],
            seen: vec![],
            own: vec![],
            exposed_until: vec![],
            hostility: vec![],
            last_decay: 0,
            last_build: 0,
            build_actions: 0,
            active_policy: 0,
            policy_gain: [0; 3],
            policy_trials: [0; 3],
            policy_started: 0,
            policy_land: 0,
            rival_momentum: vec![],
            last_land: vec![],
            last_model_tick: 0,
            tick: 0,
        }
    }

    fn index_for(radius: i32, width: usize, h: Hex) -> Option<usize> {
        (h.q.abs() <= radius && h.r.abs() <= radius && (h.q + h.r).abs() <= radius)
            .then(|| (h.r + radius) as usize * width + (h.q + radius) as usize)
    }

    fn index(&self, h: Hex) -> Option<usize> {
        Self::index_for(self.radius, self.width, h)
    }

    fn init(&mut self, view: &View) {
        self.radius = view.radius;
        self.width = (2 * view.radius + 1) as usize;
        let slots = self.width * self.width;
        self.hexes = (0..slots)
            .map(|i| {
                Hex::new(
                    (i % self.width) as i32 - view.radius,
                    (i / self.width) as i32 - view.radius,
                )
            })
            .collect();
        self.terrain = view.public_terrain.clone();
        self.neighbors = self
            .hexes
            .iter()
            .map(|h| {
                h.neighbors()
                    .into_iter()
                    .filter_map(|n| Self::index_for(view.radius, self.width, n))
                    .filter(|&i| self.terrain[i].is_some_and(|t| t.passable()))
                    .collect()
            })
            .collect();
        self.seen = vec![None; slots];
        self.own = vec![false; slots];
        self.exposed_until = vec![0; slots];
        self.hostility = vec![0; view.public_players.len()];
        self.rival_momentum = vec![0; view.public_players.len()];
        self.last_land = vec![0; view.public_players.len()];
    }

    fn remember(&mut self, view: &View) {
        if let Some(rules) = &view.rules {
            self.rules = rules.clone();
        }
        self.tick = view.tick;
        if self.radius != view.radius {
            self.init(view);
        }
        if self.hostility.len() < view.public_players.len() {
            self.hostility.resize(view.public_players.len(), 0);
        }
        // Owned cells are always visible. If an old owned cell vanished, it was lost.
        for i in 0..self.own.len() {
            if self.own[i] {
                self.own[i] = false;
                if let Some(cell) = self.seen[i].as_mut() {
                    cell.owner = None;
                }
            }
        }
        for visible in &view.visible_hexes {
            let Some(i) = self.index(visible.hex) else {
                continue;
            };
            self.seen[i] = Some(Cell {
                owner: visible.owner,
                troops: visible.troops,
                wall: visible.wall_milli,
                building: visible.building.clone(),
            });
            self.own[i] = visible.owner == Some(view.you);
        }
        for event in &view.visible_events {
            if let Event::Moved {
                player,
                from,
                troops,
                ..
            } = event
            {
                if *player != view.you && *troops >= self.k.logistics_min {
                    if let Some(i) = self.index(*from) {
                        self.exposed_until[i] = view.tick + 8;
                    }
                }
            }
            if let Event::Captured {
                player,
                previous_owner,
                ..
            } = event
            {
                let p = *player as usize;
                if p < self.hostility.len() && *player != view.you {
                    self.hostility[p] += if *previous_owner == Some(view.you) {
                        12
                    } else {
                        1
                    };
                }
            }
        }
        if view.tick >= self.last_decay + 100 {
            for heat in &mut self.hostility {
                *heat = *heat * 3 / 4;
            }
            self.last_decay = view.tick;
        }
        let land = self.land_counts(view.public_players.len());
        if view.tick >= self.last_model_tick + 50 {
            for player in 0..land.len() {
                let delta = land[player] - self.last_land[player];
                self.rival_momentum[player] = (self.rival_momentum[player] * 2 + delta * 3) / 3;
            }
            self.last_land.clone_from(&land);
            self.last_model_tick = view.tick;
        }
        if self.k.doctrine == 7 {
            let ours = land.get(view.you as usize).copied().unwrap_or(0);
            if self.policy_started == 0 {
                self.policy_started = view.tick;
                self.policy_land = ours;
            } else if view.tick >= self.policy_started + self.k.adaptive_period {
                let policy = self.active_policy;
                let gain = ours - self.policy_land;
                self.policy_gain[policy] = if self.policy_trials[policy] == 0 {
                    gain
                } else {
                    (self.policy_gain[policy] * 2 + gain) / 3
                };
                self.policy_trials[policy] += 1;
                self.active_policy = self
                    .policy_trials
                    .iter()
                    .position(|trials| *trials == 0)
                    .unwrap_or_else(|| {
                        (0..3)
                            .max_by_key(|&candidate| {
                                (self.policy_gain[candidate], std::cmp::Reverse(candidate))
                            })
                            .unwrap_or(0)
                    });
                self.policy_started = view.tick;
                self.policy_land = ours;
            }
        }
    }

    fn height(&self, i: usize) -> i8 {
        self.terrain[i].map_or(0, |t| t.height)
    }

    fn enemy(&self, i: usize, me: PlayerId) -> Option<PlayerId> {
        self.seen[i].as_ref()?.owner.filter(|&owner| owner != me)
    }

    fn loss(&self, from: usize, to: usize) -> u64 {
        let Some(target) = &self.seen[to] else {
            return u64::MAX / 4;
        };
        let terrain =
            self.rules
                .height_multiplier_milli(self.height(from), self.height(to)) as u64;
        let defense = (terrain * target.wall as u64 / MILLI as u64)
            .min(self.rules.maximum_total_defense_milli as u64);
        let win = self.rules.attacker_win_milli(self.tick) as u64;
        target.troops as u64 * defense * (MILLI as u64 - win) / win / MILLI as u64
    }

    fn adjacent_threat(&self, at: usize, omit: usize, me: PlayerId) -> u64 {
        self.neighbors[at]
            .iter()
            .copied()
            .filter(|&n| n != omit)
            .filter_map(|n| {
                self.enemy(n, me)
                    .and_then(|_| self.seen[n].as_ref().map(|cell| cell.troops as u64))
            })
            .max()
            .unwrap_or(0)
    }

    fn reserve(&self, at: usize, omit: usize, me: PlayerId) -> u64 {
        let threat = self.adjacent_threat(at, omit, me);
        if threat == 0 {
            0
        } else {
            self.k.guard + threat * self.k.hold_pct / 100
        }
    }

    /// Count public, passable room behind a target without walking through our current land.
    fn open_space(&self, start: usize) -> i64 {
        let mut distance = vec![u8::MAX; self.seen.len()];
        let mut queue = VecDeque::from([start]);
        distance[start] = 0;
        let mut room = 0;
        while let Some(i) = queue.pop_front() {
            let d = distance[i];
            if d >= 3 {
                continue;
            }
            for &n in &self.neighbors[i] {
                if distance[n] != u8::MAX || self.own[n] {
                    continue;
                }
                distance[n] = d + 1;
                queue.push_back(n);
                room += if self.seen[n].is_none() { 2 } else { 1 };
            }
        }
        room
    }

    fn land_counts(&self, players: usize) -> Vec<i64> {
        let mut land = vec![0; players];
        for cell in self.seen.iter().flatten() {
            if let Some(owner) = cell.owner {
                if (owner as usize) < land.len() {
                    land[owner as usize] += 1;
                }
            }
        }
        land
    }

    /// Whether to enter dedicated closeout play, the survivor to hunt, and public-terrain
    /// distance to its last known land. Distances remain useful after fog hides the target:
    /// captures that shorten the route keep the front moving toward the survivor.
    fn finish_context(&self, view: &View, land: &[i64]) -> (bool, Option<PlayerId>, Vec<u16>) {
        let me = view.you as usize;
        let living = view
            .public_players
            .iter()
            .filter(|player| player.alive)
            .count();
        let target = view
            .public_players
            .iter()
            .filter(|player| player.alive && player.id != view.you)
            .min_by_key(|player| {
                let remembered = land.get(player.id as usize).copied().unwrap_or(0);
                let contact = self
                    .seen
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| {
                        cell.as_ref().and_then(|cell| cell.owner) == Some(player.id)
                    })
                    .filter(|(i, _)| self.neighbors[*i].iter().any(|&n| self.own[n]))
                    .count() as i64;
                let troops = self
                    .seen
                    .iter()
                    .flatten()
                    .filter(|cell| cell.owner == Some(player.id))
                    .map(|cell| cell.troops as i64)
                    .sum::<i64>();
                match self.k.finish_target_mode {
                    // Prefer a small rival we can attack from several pads right now.
                    1 => (
                        remembered.max(1) * 100 / (contact + 1),
                        remembered,
                        player.id,
                    ),
                    // Prefer the smallest remembered army, then its land.
                    2 => (troops.max(1), remembered, player.id),
                    _ => (remembered, remembered, player.id),
                }
            })
            .map(|player| player.id);
        let ours = land.get(me).copied().unwrap_or(0);
        let rival_max = view
            .public_players
            .iter()
            .filter(|player| player.alive && player.id != view.you)
            .map(|player| land.get(player.id as usize).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let largest = ours >= 50 && ours * 100 >= rival_max * self.k.finish_ratio_pct;
        let finishing = target.is_some()
            && (living <= self.k.finish_alive || view.tick >= self.k.finish_tick || largest);

        let mut distance = vec![u16::MAX; self.seen.len()];
        if finishing {
            let mut queue = VecDeque::new();
            for (i, cell) in self.seen.iter().enumerate() {
                if cell.as_ref().and_then(|cell| cell.owner) == target {
                    distance[i] = 0;
                    queue.push_back(i);
                }
            }
            while let Some(i) = queue.pop_front() {
                let next = distance[i].saturating_add(1);
                for &n in &self.neighbors[i] {
                    if distance[n] == u16::MAX {
                        distance[n] = next;
                        queue.push_back(n);
                    }
                }
            }
        }
        (finishing, target, distance)
    }

    fn political_value(
        &self,
        view: &View,
        owner: PlayerId,
        land: &[i64],
        leader: Option<PlayerId>,
    ) -> i64 {
        let p = owner as usize;
        let their_land = land.get(p).copied().unwrap_or(0);
        let living = view.public_players.iter().filter(|p| p.alive).count();
        let endgame = living <= 3 || view.tick >= self.k.aggression_tick;
        let doctrine = if self.k.doctrine == 7 {
            [1, 4, 5][self.active_policy]
        } else {
            self.k.doctrine
        };
        match doctrine {
            // Cartographer: defer wars while neutral room remains, then balance the leader.
            0 => {
                if !endgame && view.tick < self.k.peace_until {
                    -self.k.enemy
                } else if Some(owner) == leader {
                    self.k.leader
                } else {
                    self.k.enemy / 2
                }
            }
            // Vulture: collapse the smallest visible polity before it can recover.
            1 => {
                let smallest = land
                    .iter()
                    .enumerate()
                    .filter(|(i, n)| **n > 0 && *i != view.you as usize)
                    .map(|(_, n)| *n)
                    .min()
                    .unwrap_or(their_land);
                self.k.enemy
                    + if their_land <= smallest + 3 {
                        self.k.prey
                    } else {
                        0
                    }
            }
            // Steward: long peace for compounding, but decisive leader containment late.
            2 => {
                if !endgame && view.tick < self.k.peace_until {
                    -self.k.prey
                } else if Some(owner) == leader {
                    self.k.leader
                } else {
                    0
                }
            }
            // Avenger: aggression creates a persistent debt; otherwise avoid making enemies.
            3 => {
                self.k.enemy
                    + self.hostility.get(p).copied().unwrap_or(0) * self.k.revenge
                    + if Some(owner) == leader {
                        self.k.leader / 2
                    } else {
                        0
                    }
            }
            // Breaker: seek the strongest rival and turn its loss into a two-sided swing.
            4 => {
                self.k.enemy
                    + if Some(owner) == leader {
                        self.k.leader
                    } else {
                        0
                    }
                    + their_land / 2
            }
            // Sheriff: police whichever neighbour is visibly gaining land fastest.
            5 => {
                let hottest = self.hostility.iter().copied().max().unwrap_or(0);
                let heat = self.hostility.get(p).copied().unwrap_or(0);
                if heat > 0 && heat == hottest {
                    self.k.revenge * heat
                } else if endgame {
                    self.k.enemy
                } else {
                    -self.k.prey
                }
            }
            // Futures trader: attack the rival whose remembered territory has recently
            // grown fastest, before that compounding action economy becomes the leader.
            8 => {
                self.k.enemy
                    + self.rival_momentum.get(p).copied().unwrap_or(0) * self.k.momentum_weight
                    + if Some(owner) == leader {
                        self.k.leader / 3
                    } else {
                        0
                    }
            }
            // Portfolio: choose a diplomatic role from the board instead of committing
            // to one at construction time: finish a weak polity, contain a runaway, or
            // punish the locally aggressive player, in that order.
            _ => {
                let ours = land.get(view.you as usize).copied().unwrap_or(1).max(1);
                let smallest = land
                    .iter()
                    .enumerate()
                    .filter(|(i, n)| **n > 0 && *i != view.you as usize)
                    .map(|(_, n)| *n)
                    .min()
                    .unwrap_or(their_land);
                let heat = self.hostility.get(p).copied().unwrap_or(0);
                let hottest = self.hostility.iter().copied().max().unwrap_or(0);
                if their_land <= smallest + 2 && their_land <= 30 {
                    self.k.prey
                } else if Some(owner) == leader && their_land * 100 >= ours * 125 {
                    self.k.leader
                } else if heat >= 4 && heat == hottest {
                    self.k.revenge * heat
                } else if endgame {
                    self.k.enemy
                } else {
                    -self.k.enemy
                }
            }
        }
    }

    /// Rival land that would surrender if `to` became ours. Unknown land and any contact
    /// with impassable terrain make a component open, matching the engine's encirclement
    /// rule. Searches are bounded so a huge empire cannot consume an action's time budget.
    fn enclosure_gain(&self, to: usize, me: PlayerId) -> i64 {
        const LIMIT: usize = 64;
        let mut total = 0;
        let mut done: Vec<usize> = vec![];
        for &seed in &self.neighbors[to] {
            if done.contains(&seed) {
                continue;
            }
            let Some(owner) = self.enemy(seed, me) else {
                continue;
            };
            let mut component = vec![seed];
            let mut cursor = 0;
            let mut enclosed = true;
            while cursor < component.len() && enclosed {
                let current = component[cursor];
                cursor += 1;
                for neighbor_hex in self.hexes[current].neighbors() {
                    let Some(n) = self
                        .index(neighbor_hex)
                        .filter(|&i| self.terrain[i].is_some_and(|terrain| terrain.passable()))
                    else {
                        enclosed = false;
                        break;
                    };
                    if n == to || self.own[n] {
                        continue;
                    }
                    match self.seen[n].as_ref().and_then(|cell| cell.owner) {
                        Some(other) if other == owner => {
                            if !component.contains(&n) {
                                component.push(n);
                            }
                        }
                        _ => {
                            enclosed = false;
                            break;
                        }
                    }
                }
                if component.len() > LIMIT {
                    enclosed = false;
                }
            }
            done.extend(&component);
            if enclosed {
                total += component.len() as i64;
            }
        }
        total
    }

    fn bid(
        &self,
        view: &View,
        from: usize,
        to: usize,
        land: &[i64],
        leader: Option<PlayerId>,
        finishing: bool,
        finish_target: Option<PlayerId>,
        hunt_distance: &[u16],
    ) -> Bid {
        let me = view.you;
        let loss = self.loss(from, to);
        let own_land = self.own.iter().filter(|owned| **owned).count();
        let opening = own_land <= self.k.opening_land;
        let living = view
            .public_players
            .iter()
            .filter(|player| player.alive)
            .count();
        let endgame = living <= 3 || view.tick >= self.k.aggression_tick;
        let target_hold = if opening {
            0
        } else {
            self.reserve(to, from, me)
        };
        let source_hold = if opening {
            0
        } else {
            self.reserve(from, to, me)
        };
        let margin = if endgame {
            self.k.endgame_margin_pct
        } else {
            self.k.margin_pct
        };
        let need = loss * margin / 100 + self.k.margin_abs + target_hold;
        let source = self.seen[from].as_ref().expect("own source is visible");
        let available = (source.troops as u64).saturating_sub(1 + source_hold);
        let short = need.saturating_sub(available);
        let send = if short == 0 {
            let source_exposed = self.neighbors[from]
                .iter()
                .any(|&n| n != to && !self.own[n]);
            let forward = if source_exposed {
                self.k.forward_pct
            } else {
                100
            };
            (need + (available - need) * forward / 100).min(source.troops as u64 - 1) as u32
        } else {
            0
        };
        let own_neighbors = self.neighbors[to].iter().filter(|&&n| self.own[n]).count() as i64;
        let foreign_neighbors = self.neighbors[to].len() as i64 - own_neighbors;
        let owner = self.enemy(to, me);
        let mut score =
            self.k.neutral + self.k.space * self.open_space(to) + self.k.hug * own_neighbors
                - self.k.hug * foreign_neighbors / 3
                + self.k.high * self.height(to) as i64
                - loss as i64 * self.k.cost / 100;
        if let Some(owner) = owner {
            score += self.political_value(view, owner, land, leader);
            if finishing {
                score += if Some(owner) == finish_target {
                    self.k.finish_bonus
                } else {
                    -self.k.finish_bonus / 2
                };
            }
        } else if finishing && hunt_distance[to] != u16::MAX {
            score -= self.k.hunt_value * hunt_distance[to] as i64;
        }
        if self.exposed_until[to] >= view.tick {
            score += self.k.counter_bonus;
        }
        if owner.is_some() && self.k.enclose_value != 0 {
            score += self.k.enclose_value * self.enclosure_gain(to, me);
        }
        // A source sealed into the interior after this capture is especially efficient.
        if self.neighbors[from]
            .iter()
            .all(|&n| n == to || self.own[n] || !self.terrain[n].is_some_and(|t| t.passable()))
        {
            score += self.k.hug * 3;
        }
        if send > 0 && self.k.lookahead_value != 0 {
            let survivors = send as u64 - loss.min(send as u64);
            let followups = self.neighbors[to]
                .iter()
                .copied()
                .filter(|&n| !self.own[n] && n != from && self.seen[n].is_some())
                .filter(|&n| {
                    let next_loss = self.loss(to, n);
                    survivors
                        > next_loss * margin / 100 + self.k.margin_abs + self.reserve(n, to, me)
                })
                .count() as i64;
            score += self.k.lookahead_value * followups;
        }
        Bid {
            score,
            from,
            to,
            send,
            short,
        }
    }

    fn bids(&self, view: &View) -> (Option<Bid>, Option<Bid>) {
        let land = self.land_counts(view.public_players.len());
        let (finishing, finish_target, hunt_distance) = self.finish_context(view, &land);
        let leader = land
            .iter()
            .enumerate()
            .filter(|(p, n)| **n > 0 && *p != view.you as usize)
            .max_by_key(|(p, n)| (**n, std::cmp::Reverse(*p)))
            .map(|(p, _)| p as PlayerId);
        let mut ready: Option<Bid> = None;
        let mut blocked: Option<Bid> = None;
        for from in 0..self.own.len() {
            if !self.own[from] {
                continue;
            }
            for &to in &self.neighbors[from] {
                if self.own[to] || self.seen[to].is_none() {
                    continue;
                }
                let bid = self.bid(
                    view,
                    from,
                    to,
                    &land,
                    leader,
                    finishing,
                    finish_target,
                    &hunt_distance,
                );
                if bid.short == 0 {
                    if ready.is_none_or(|best| {
                        (bid.score, bid.send, std::cmp::Reverse(bid.to))
                            > (best.score, best.send, std::cmp::Reverse(best.to))
                    }) {
                        ready = Some(bid);
                    }
                } else if blocked.is_none_or(|best| {
                    (bid.score, std::cmp::Reverse(bid.short))
                        > (best.score, std::cmp::Reverse(best.short))
                }) {
                    blocked = Some(bid);
                }
            }
        }
        (ready, blocked)
    }

    fn management(&self) -> Vec<ManagementCommand> {
        // Farm adjacency is automatic in v0.5; only Wall links remain meaningful.
        for (i, cell) in self.seen.iter().enumerate() {
            let Some(cell) = cell.as_ref().filter(|_| self.own[i]) else {
                continue;
            };
            let Some(building) = &cell.building else {
                continue;
            };
            if building.link_cooldown > 0 {
                continue;
            }
            if building.kind == Building::Wall
                && building.links.len() < self.rules.wall.max_links as usize
            {
                for &n in &self.neighbors[i] {
                    let Some(other) = self.seen[n].as_ref().and_then(|c| c.building.as_ref())
                    else {
                        continue;
                    };
                    if self.own[n]
                        && other.kind == Building::Wall
                        && other.link_cooldown == 0
                        && !building.links.contains(&self.hexes[n])
                        && other.links.len() < self.rules.wall.max_links as usize
                    {
                        return vec![ManagementCommand::SetWallLink {
                            wall_a: self.hexes[i],
                            wall_b: self.hexes[n],
                            enabled: true,
                        }];
                    }
                }
            }
        }
        vec![]
    }

    fn build(&self, view: &View) -> Option<Action> {
        if self.k.build_mode == 0
            || self.build_actions >= self.k.build_cap
            || view.tick < self.k.build_start
            || view.tick >= self.k.build_stop
            || view.tick < self.last_build + self.k.build_every
        {
            return None;
        }
        let mode = self.k.build_mode;
        let quarries = self
            .seen
            .iter()
            .enumerate()
            .filter(|(i, cell)| {
                self.own[*i]
                    && cell.as_ref().is_some_and(|cell| {
                        cell.building
                            .as_ref()
                            .is_some_and(|building| building.kind == Building::Quarry)
                    })
            })
            .count();
        let mut best: Option<(i64, usize, Building)> = None;
        for i in 0..self.own.len() {
            let Some(cell) = self.seen[i]
                .as_ref()
                .filter(|c| self.own[i] && c.building.is_none())
            else {
                continue;
            };
            let exposed = self.neighbors[i]
                .iter()
                .any(|&n| self.enemy(n, view.you).is_some());
            let own_neighbors = self.neighbors[i].iter().filter(|&&n| self.own[n]).count() as i64;
            let adjacent_kind = |kind: Building| {
                self.neighbors[i]
                    .iter()
                    .filter(|&&n| {
                        self.own[n]
                            && self.seen[n].as_ref().is_some_and(|cell| {
                                cell.building
                                    .as_ref()
                                    .is_some_and(|building| building.kind == kind)
                            })
                    })
                    .count() as i64
            };
            let adjacent_farms = adjacent_kind(Building::Farm);
            let adjacent_villages = adjacent_kind(Building::Village);
            let adjacent_quarries = adjacent_kind(Building::Quarry);
            let potential_farms = self.neighbors[i]
                .iter()
                .filter(|&&n| {
                    self.own[n]
                        && self.seen[n]
                            .as_ref()
                            .is_some_and(|cell| cell.building.is_none())
                        && self.rules.terrain_allowed(Building::Farm, self.height(n))
                })
                .count() as i64;
            let choices: &[Building] = match mode {
                1 => &[Building::Farm, Building::Village],
                2 => &[Building::Farm, Building::Village, Building::Quarry],
                3 => &[Building::Village],
                4 => &[Building::Farm, Building::Village, Building::Wall],
                _ => &[
                    Building::Farm,
                    Building::Village,
                    Building::Quarry,
                    Building::Wall,
                ],
            };
            for &kind in choices {
                if !self.rules.terrain_allowed(kind, self.height(i)) {
                    continue;
                }
                if kind == Building::Quarry {
                    let too_close = self.seen.iter().enumerate().any(|(q, cell)| {
                        self.own[q]
                            && cell.as_ref().is_some_and(|cell| {
                                cell.building
                                    .as_ref()
                                    .is_some_and(|building| building.kind == Building::Quarry)
                            })
                            && self.hexes[i].distance(self.hexes[q])
                                < self.rules.quarry.min_friendly_distance
                    });
                    if quarries >= self.k.quarry_cap || too_close {
                        continue;
                    }
                }
                let discounted = adjacent_quarries > 0;
                let cost = self.rules.build_cost(kind, discounted);
                if cell.troops <= cost.troops {
                    continue;
                }
                let adjacent_wall = self.neighbors[i].iter().any(|&n| {
                    self.own[n]
                        && self.seen[n].as_ref().is_some_and(|c| {
                            c.building
                                .as_ref()
                                .is_some_and(|b| b.kind == Building::Wall)
                        })
                });
                let score = match kind {
                    Building::Farm => {
                        if adjacent_villages == 0 {
                            continue;
                        }
                        800 * adjacent_villages + if discounted { 80 } else { 0 }
                            - if exposed { 120 } else { 0 }
                    }
                    Building::Village => {
                        let economy = match mode {
                            3 => 320,
                            2 if discounted => 650,
                            2 => 120,
                            _ => 500,
                        };
                        economy + adjacent_farms * 120 + potential_farms * 35 + own_neighbors * 8
                            - if exposed { 160 } else { 0 }
                    }
                    Building::Wall => {
                        (if exposed { 260 } else { 20 }) + if adjacent_wall { 100 } else { 0 }
                    }
                    Building::Quarry => {
                        600 + own_neighbors * 25 + self.height(i) as i64 * 20
                            - if exposed { 180 } else { 0 }
                    }
                };
                if best.is_none_or(|current| score > current.0) {
                    best = Some((score, i, kind));
                }
            }
        }
        best.map(|(_, i, building)| Action::Build {
            at: self.hexes[i],
            building,
        })
    }

    /// Move a remote interior stack one friendly step toward a blocked launch pad.
    fn logistics(&self, pad: usize, short: u64, me: PlayerId) -> Option<Action> {
        let mut parent = vec![usize::MAX; self.own.len()];
        let mut distance = vec![u32::MAX; self.own.len()];
        let mut queue = VecDeque::from([pad]);
        parent[pad] = pad;
        distance[pad] = 0;
        let mut source: Option<(u64, usize, u64)> = None;
        while let Some(i) = queue.pop_front() {
            if i != pad {
                let troops = self.seen[i].as_ref().map_or(0, |cell| cell.troops) as u64;
                let spare = troops.saturating_sub(1 + self.reserve(i, usize::MAX, me));
                if spare >= self.k.relay_floor {
                    let value = spare.min(short.max(1)) * 1000 / distance[i].max(1) as u64;
                    if source.is_none_or(|best| {
                        (value, spare, std::cmp::Reverse(i))
                            > (best.0, best.2, std::cmp::Reverse(best.1))
                    }) {
                        source = Some((value, i, spare));
                    }
                }
            }
            for &n in &self.neighbors[i] {
                if self.own[n] && parent[n] == usize::MAX {
                    parent[n] = i;
                    distance[n] = distance[i] + 1;
                    queue.push_back(n);
                }
            }
        }
        let (_, from, troops) = source?;
        Some(Action::Move {
            from: self.hexes[from],
            to: self.hexes[parent[from]],
            troops: troops as u32,
        })
    }
}

impl Bot for Mosaic {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        self.remember(view);
        let management = self.management();
        let (ready, blocked) = self.bids(view);
        let relay = blocked.filter(|blocked_bid| {
            ready
                .is_none_or(|ready_bid| blocked_bid.score > ready_bid.score + self.k.relay_patience)
        });
        let wave_pad = blocked.or(ready);
        let charging = self.k.wave_cycle > 0
            && view.tick % self.k.wave_cycle < self.k.wave_charge.min(self.k.wave_cycle)
            && self.own.iter().filter(|owned| **owned).count() > self.k.opening_land;
        let action = if charging && wave_pad.is_some() {
            let bid = wave_pad.expect("checked above");
            self.logistics(bid.from, bid.short, view.you)
                .or_else(|| {
                    self.k.allow_gather.then(|| Action::Gather {
                        target: self.hexes[bid.from],
                    })
                })
                .unwrap_or(Action::NoOp)
        } else if let Some(bid) = relay {
            self.logistics(bid.from, bid.short, view.you)
                .or_else(|| {
                    self.k.allow_gather.then(|| Action::Gather {
                        target: self.hexes[bid.from],
                    })
                })
                .unwrap_or(Action::NoOp)
        } else if let Some(build) = self.build(view) {
            self.last_build = view.tick;
            self.build_actions += 1;
            build
        } else if let Some(bid) = ready {
            Action::Move {
                from: self.hexes[bid.from],
                to: self.hexes[bid.to],
                troops: bid.send,
            }
        } else if let Some(bid) = blocked {
            self.logistics(bid.from, bid.short, view.you)
                .or_else(|| {
                    self.k.allow_gather.then(|| Action::Gather {
                        target: self.hexes[bid.from],
                    })
                })
                .unwrap_or(Action::NoOp)
        } else {
            Action::NoOp
        };
        PlayerCommand { management, action }
    }
}
