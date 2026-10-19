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
    neutral: i64,
    enemy: i64,
    space: i64,
    hug: i64,
    high: i64,
    cost: i64,
    leader: i64,
    prey: i64,
    revenge: i64,
    peace_until: u32,
    build_mode: i64,
    build_every: u32,
    build_cap: usize,
    logistics_min: u32,
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
            neutral: k("neutral", 120),
            enemy: k("enemy", 35),
            space: k("space", 9),
            hug: k("hug", 15),
            high: k("high", 8),
            cost: k("cost", 35),
            leader: k("leader", 80),
            prey: k("prey", 90),
            revenge: k("revenge", 18),
            peace_until: k("peace_until", 500).max(0) as u32,
            build_mode: k("build_mode", 0),
            build_every: k("build_every", 11).max(1) as u32,
            build_cap: k("build_cap", 8).max(0) as usize,
            logistics_min: k("logistics_min", 80).max(2) as u32,
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
    hostility: Vec<i64>,
    last_decay: u32,
    last_build: u32,
    build_actions: usize,
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
            hostility: vec![],
            last_decay: 0,
            last_build: 0,
            build_actions: 0,
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
        self.hostility = vec![0; view.public_players.len()];
    }

    fn remember(&mut self, view: &View) {
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
        let win = self.rules.attacker_exchange_win_milli as u64;
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
        let endgame = living <= 3 || view.tick >= 1900;
        match self.k.doctrine {
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

    fn bid(
        &self,
        view: &View,
        from: usize,
        to: usize,
        land: &[i64],
        leader: Option<PlayerId>,
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
        let endgame = living <= 3 || view.tick >= 2200;
        let target_hold = if opening || endgame {
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
            // Interior launch pads transfer all surplus; exposed pads preserve their reserve.
            available.max(need).min(source.troops as u64 - 1) as u32
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
        }
        // A source sealed into the interior after this capture is especially efficient.
        if self.neighbors[from]
            .iter()
            .all(|&n| n == to || self.own[n] || !self.terrain[n].is_some_and(|t| t.passable()))
        {
            score += self.k.hug * 3;
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
                let bid = self.bid(view, from, to, &land, leader);
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
        // Link one productive pair per tick. A single command avoids invalidating a second
        // command through the same building's cooldown.
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
            if building.kind == Building::Farm
                && building.links.len() < self.rules.farm.sockets as usize
            {
                for &n in &self.neighbors[i] {
                    let Some(other) = self.seen[n].as_ref().and_then(|c| c.building.as_ref())
                    else {
                        continue;
                    };
                    if self.own[n] && other.kind == Building::Village && other.links.is_empty() {
                        return vec![ManagementCommand::SetFarmLink {
                            farm: self.hexes[i],
                            village: self.hexes[n],
                            enabled: true,
                        }];
                    }
                }
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
            || view.tick < self.last_build + self.k.build_every
        {
            return None;
        }
        let resource = view.own_economy.resource;
        let mode = self.k.build_mode;
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
            let choices: &[Building] = match mode {
                1 => &[Building::Farm, Building::Village],
                2 => &[Building::Wall],
                _ => &[Building::Farm, Building::Village, Building::Quarry],
            };
            for &kind in choices {
                if !self.rules.terrain_allowed(kind, self.height(i)) {
                    continue;
                }
                let cost = self.rules.build_cost(kind, false);
                if cell.troops <= cost.troops || resource < cost.resource {
                    continue;
                }
                let adjacent_farm = self.neighbors[i].iter().any(|&n| {
                    self.own[n]
                        && self.seen[n].as_ref().is_some_and(|c| {
                            c.building.as_ref().is_some_and(|b| {
                                b.kind == Building::Farm
                                    && b.links.len() < self.rules.farm.sockets as usize
                            })
                        })
                });
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
                        100 + self.neighbors[i].iter().filter(|&&n| self.own[n]).count() as i64 * 8
                            - if exposed { 80 } else { 0 }
                    }
                    Building::Village => {
                        (if adjacent_farm { 180 } else { 20 }) + if exposed { -60 } else { 0 }
                    }
                    Building::Wall => {
                        (if exposed { 150 } else { 10 }) + if adjacent_wall { 80 } else { 0 }
                    }
                    Building::Quarry => 70 + self.height(i) as i64 * 20,
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
    fn logistics(&self, pad: usize) -> Option<Action> {
        let mut parent = vec![usize::MAX; self.own.len()];
        let mut distance = vec![u32::MAX; self.own.len()];
        let mut queue = VecDeque::from([pad]);
        parent[pad] = pad;
        distance[pad] = 0;
        let mut source: Option<usize> = None;
        while let Some(i) = queue.pop_front() {
            if i != pad {
                let troops = self.seen[i].as_ref().map_or(0, |cell| cell.troops);
                if troops >= self.k.logistics_min
                    && source.is_none_or(|s| {
                        (troops / distance[i].max(1), std::cmp::Reverse(i))
                            > (
                                self.seen[s].as_ref().map_or(0, |cell| cell.troops)
                                    / distance[s].max(1),
                                std::cmp::Reverse(s),
                            )
                    })
                {
                    source = Some(i);
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
        let from = source?;
        let troops = self.seen[from].as_ref()?.troops.saturating_sub(1);
        (troops > 0).then(|| Action::Move {
            from: self.hexes[from],
            to: self.hexes[parent[from]],
            troops,
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
        let action = if let Some(build) = self.build(view) {
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
            self.logistics(bid.from).unwrap_or(Action::Gather {
                target: self.hexes[bid.from],
            })
        } else {
            Action::NoOp
        };
        PlayerCommand { management, action }
    }
}
