//! Pure deterministic Hexarena rules engine. No I/O, clocks, or global state.

pub mod mapgen;
pub mod rng;
pub mod scoring;

use hexarena_protocol::*;
use mapgen::{generate, passable};

#[derive(Clone, Copy, Debug)]
pub struct Board {
    pub radius: i32,
}

impl Board {
    pub fn new(radius: i32) -> Self {
        Board { radius }
    }
    pub fn width(&self) -> i32 {
        2 * self.radius + 1
    }
    pub fn slots(&self) -> usize {
        (self.width() * self.width()) as usize
    }
    pub fn contains(&self, h: Hex) -> bool {
        h.q.abs() <= self.radius && h.r.abs() <= self.radius && (h.q + h.r).abs() <= self.radius
    }
    pub fn index(&self, h: Hex) -> Option<usize> {
        self.contains(h)
            .then(|| ((h.r + self.radius) * self.width() + h.q + self.radius) as usize)
    }
    /// All on-board hexes sorted by (r, q).
    pub fn hexes(&self) -> Vec<Hex> {
        let r = self.radius;
        let mut v = vec![];
        for rr in -r..=r {
            for q in (-r).max(-rr - r)..=r.min(-rr + r) {
                v.push(Hex::new(q, rr));
            }
        }
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellState {
    pub owner: Option<PlayerId>,
    pub troops: u32,
    pub building: Option<Building>,
}

#[derive(Clone, Debug)]
pub struct State {
    pub settings: MatchSettings,
    pub board: Board,
    pub terrain: Vec<Option<Terrain>>,
    pub cells: Vec<CellState>,
    pub names: Vec<String>,
    pub resources: Vec<u32>,
    pub eliminated: Vec<bool>,
    pub eliminated_at: Vec<Option<u32>>,
    pub tick: u32,
    pub last_events: Vec<Event>,
    pub result: Option<MatchResult>,
}

impl State {
    pub fn new(settings: MatchSettings, names: Vec<String>) -> Result<State, String> {
        let map = generate(&settings)?;
        let board = Board::new(map.radius);
        let mut cells = vec![CellState::default(); board.slots()];
        for (i, c) in cells.iter_mut().enumerate() {
            c.troops = map.garrison[i];
            c.building = map.buildings[i];
        }
        for (p, &sp) in map.spawns.iter().enumerate() {
            let c = &mut cells[board.index(sp).unwrap()];
            c.owner = Some(p as PlayerId);
            c.troops = settings.rules.start_troops;
            c.building = None;
        }
        let n = settings.players as usize;
        let mut names = names;
        names.resize(n, String::new());
        for (i, nm) in names.iter_mut().enumerate() {
            if nm.is_empty() {
                *nm = format!("Player {}", i + 1);
            }
        }
        Ok(State {
            settings,
            board,
            terrain: map.terrain,
            cells,
            names,
            resources: vec![0; n],
            eliminated: vec![false; n],
            eliminated_at: vec![None; n],
            tick: 0,
            last_events: vec![],
            result: None,
        })
    }

    pub fn rules(&self) -> &RulesConfig {
        &self.settings.rules
    }

    pub fn cell(&self, h: Hex) -> Option<&CellState> {
        self.board.index(h).map(|i| &self.cells[i])
    }

    pub fn is_over(&self) -> bool {
        self.result.is_some()
    }

    pub fn hex_counts(&self) -> Vec<u32> {
        let mut v = vec![0; self.settings.players as usize];
        for c in &self.cells {
            if let Some(o) = c.owner {
                v[o as usize] += 1;
            }
        }
        v
    }

    /// Each Farm links to its nearest owned Village within range.
    pub fn farm_links(&self, player: PlayerId) -> Vec<(Hex, Hex)> {
        let hexes = self.board.hexes();
        let owned = |h: Hex, b: Building| {
            let c = self.cell(h).unwrap();
            c.owner == Some(player) && c.building == Some(b)
        };
        let villages: Vec<Hex> = hexes
            .iter()
            .copied()
            .filter(|&h| owned(h, Building::Village))
            .collect();
        let range = self.rules().farm_village_link_range;
        hexes
            .iter()
            .copied()
            .filter(|&h| owned(h, Building::Farm))
            .filter_map(|f| {
                villages
                    .iter()
                    .copied()
                    .filter(|v| v.distance(f) <= range)
                    .min_by_key(|v| (v.distance(f), *v))
                    .map(|v| (f, v))
            })
            .collect()
    }

    fn validate(&self, p: PlayerId, a: &Action) -> Result<(), &'static str> {
        match a {
            Action::Pass => Ok(()),
            Action::Move { from, to, troops } => {
                let src = self.cell(*from).ok_or("source off board")?;
                if src.owner != Some(p) {
                    return Err("source not owned");
                }
                if from.distance(*to) != 1 {
                    return Err("target not adjacent");
                }
                if !passable(&self.board, &self.terrain, *to) {
                    return Err("target impassable");
                }
                if *troops == 0 || *troops >= src.troops {
                    return Err("must move at least 1 and leave at least 1 troop");
                }
                Ok(())
            }
            Action::Build { at, building } => {
                let c = self.cell(*at).ok_or("hex off board")?;
                if c.owner != Some(p) {
                    return Err("hex not owned");
                }
                if c.building.is_some() {
                    return Err("hex already has a building");
                }
                if self.resources[p as usize] < self.rules().cost(*building) {
                    return Err("insufficient resources");
                }
                Ok(())
            }
        }
    }

    fn apply_move(&mut self, p: PlayerId, from: Hex, to: Hex, n: u32, ev: &mut Vec<Event>) {
        let (fi, ti) = (
            self.board.index(from).unwrap(),
            self.board.index(to).unwrap(),
        );
        self.cells[fi].troops -= n;
        ev.push(Event::Moved {
            player: p,
            from,
            to,
            troops: n,
        });
        let target = self.cells[ti];
        if target.owner == Some(p) {
            self.cells[ti].troops += n;
            return;
        }
        let height = self.terrain[ti].map(|t| t.height).unwrap_or(0);
        let mut pct = self.rules().defense_pct_by_height[(height + 2) as usize];
        if target.building == Some(Building::Wall) {
            pct += self.rules().wall_defense_bonus_pct;
        }
        let atk = n * 100;
        let def = target.troops * pct;
        if atk > def {
            let left = ((atk - def) / 100).max(1);
            self.cells[ti].owner = Some(p);
            self.cells[ti].troops = left;
            ev.push(Event::Captured {
                player: p,
                at: to,
                previous_owner: target.owner,
                troops_left: left,
            });
        } else {
            let lost = (atk / pct).min(target.troops.saturating_sub(1));
            self.cells[ti].troops = target.troops - lost;
            ev.push(Event::Repelled {
                attacker: p,
                at: to,
                attackers_lost: n,
            });
        }
    }

    /// Advance one tick. At most one action per player is used; extras are ignored.
    pub fn step(&mut self, actions: &[(PlayerId, Action)]) {
        if self.result.is_some() {
            return;
        }
        let n = self.settings.players as usize;
        let mut events = vec![];
        let mut order: Vec<PlayerId> = (0..n as PlayerId).collect();
        order.rotate_left(self.tick as usize % n);
        for p in order {
            if self.eliminated[p as usize] {
                continue;
            }
            let Some((_, a)) = actions.iter().find(|(pid, _)| *pid == p) else {
                continue;
            };
            match self.validate(p, a) {
                Err(reason) => events.push(Event::Rejected {
                    player: p,
                    reason: reason.into(),
                }),
                Ok(()) => match a {
                    Action::Pass => {}
                    Action::Move { from, to, troops } => {
                        self.apply_move(p, *from, *to, *troops, &mut events)
                    }
                    Action::Build { at, building } => {
                        self.resources[p as usize] -= self.rules().cost(*building);
                        let i = self.board.index(*at).unwrap();
                        self.cells[i].building = Some(*building);
                        events.push(Event::Built {
                            player: p,
                            at: *at,
                            building: *building,
                        });
                    }
                },
            }
        }
        let counts = self.hex_counts();
        for p in 0..n {
            if !self.eliminated[p] && counts[p] == 0 {
                self.eliminated[p] = true;
                self.eliminated_at[p] = Some(self.tick + 1);
                events.push(Event::Eliminated {
                    player: p as PlayerId,
                });
            }
        }
        let income = self.rules().income_per_hex;
        let (qi, fi) = (self.rules().quarry_income, self.rules().farm_link_income);
        for c in self.cells.iter_mut() {
            if let Some(o) = c.owner {
                c.troops += income;
                if c.building == Some(Building::Quarry) {
                    self.resources[o as usize] += qi;
                }
            }
        }
        for p in 0..n as PlayerId {
            self.resources[p as usize] += fi * self.farm_links(p).len() as u32;
        }
        self.tick += 1;
        self.last_events = events;
        let alive: Vec<usize> = (0..n).filter(|&p| !self.eliminated[p]).collect();
        let timeout = self.tick >= self.rules().tick_limit;
        if alive.len() <= 1 || timeout {
            self.result = Some(self.final_result());
        }
    }

    /// Final standings: elimination order, then land, troops, resources, buildings.
    pub fn final_result(&self) -> MatchResult {
        let n = self.settings.players as usize;
        let counts = self.hex_counts();
        let mut troops = vec![0u32; n];
        let mut buildings = vec![0u32; n];
        for c in &self.cells {
            if let Some(o) = c.owner {
                troops[o as usize] += c.troops;
                buildings[o as usize] += c.building.is_some() as u32;
            }
        }
        let keys: Vec<scoring::SurvivorKey> = (0..n)
            .map(|p| (counts[p], troops[p], self.resources[p], buildings[p]))
            .collect();
        scoring::build_result(self.tick, &self.eliminated_at, &keys, counts)
    }

    /// Hexes the player can currently see (bitmap over board slots).
    pub fn visible_mask(&self, player: PlayerId) -> Vec<bool> {
        let r = self.rules().vision_radius;
        let mut mask = vec![false; self.board.slots()];
        for h in self.board.hexes() {
            if self.cell(h).unwrap().owner != Some(player) {
                continue;
            }
            for dq in -r..=r {
                for dr in (-r).max(-dq - r)..=r.min(-dq + r) {
                    if let Some(i) = self.board.index(Hex::new(h.q + dq, h.r + dr)) {
                        mask[i] = true;
                    }
                }
            }
        }
        mask
    }

    /// The only information channel to a player. Depends solely on public data, the
    /// player's own state, and cells within the player's vision.
    pub fn view(&self, player: PlayerId) -> View {
        let mask = self.visible_mask(player);
        let vis = |h: Hex| self.board.index(h).map(|i| mask[i]).unwrap_or(false);
        let public = self.rules().public_terrain;
        let terrain = self
            .terrain
            .iter()
            .enumerate()
            .map(|(i, t)| if public || mask[i] { *t } else { None })
            .collect();
        let visible = self
            .board
            .hexes()
            .into_iter()
            .filter(|&h| vis(h))
            .map(|h| {
                let c = self.cell(h).unwrap();
                VisibleCell {
                    hex: h,
                    owner: c.owner,
                    troops: c.troops,
                    building: c.building,
                }
            })
            .collect();
        let events = self
            .last_events
            .iter()
            .filter(|e| match e {
                Event::Moved { from, to, .. } => vis(*from) || vis(*to),
                Event::Captured { at, .. }
                | Event::Repelled { at, .. }
                | Event::Built { at, .. } => vis(*at),
                Event::Eliminated { .. } => true,
                Event::Rejected { player: p, .. } => *p == player,
            })
            .cloned()
            .collect();
        View {
            protocol_version: PROTOCOL_VERSION,
            rules_version: self.rules().rules_version.clone(),
            tick: self.tick,
            player,
            radius: self.board.radius,
            vision_radius: self.rules().vision_radius,
            players: (0..self.settings.players)
                .map(|id| PublicPlayer {
                    id,
                    name: self.names[id as usize].clone(),
                    eliminated: self.eliminated[id as usize],
                })
                .collect(),
            terrain,
            visible,
            own_resources: self.resources[player as usize],
            events,
        }
    }

    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        feed(self.tick as u64);
        for c in &self.cells {
            feed(c.owner.map(|o| o as u64 + 1).unwrap_or(0));
            feed(c.troops as u64);
            feed(c.building.map(|b| b as u64 + 1).unwrap_or(0));
        }
        for r in &self.resources {
            feed(*r as u64);
        }
        h
    }
}

/// Re-simulate a replay from its seeds and recorded actions.
pub fn replay_state(replay: &Replay) -> Result<State, String> {
    let mut st = State::new(replay.settings.clone(), replay.names.clone())?;
    for tick_actions in &replay.actions {
        st.step(tick_actions);
    }
    Ok(st)
}

#[cfg(test)]
mod tests;
