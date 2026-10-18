//! Pure deterministic Hexarena rules engine. No I/O, clocks, or global state.

pub mod mapgen;
pub mod rng;
pub mod scoring;
pub mod stats;

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
    /// On-board hexes within `r` of `c` (including `c`).
    pub fn hexes_within(&self, c: Hex, r: i32) -> Vec<Hex> {
        let mut v = vec![];
        for dq in -r..=r {
            for dr in (-r).max(-dq - r)..=r.min(-dq + r) {
                let h = Hex::new(c.q + dq, c.r + dr);
                if self.contains(h) {
                    v.push(h);
                }
            }
        }
        v
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellState {
    pub owner: Option<PlayerId>,
    pub troops: u32,
    pub building: Option<Building>,
    pub built_tick: u32,
    /// Resource production progress in ticks (Farm, Quarry).
    pub progress: u32,
    /// Farm: Village per socket. Wall: active partner Walls.
    pub links: [Option<Hex>; 3],
    pub link_ready_tick: u32,
    pub record: u32,
}

const NO_RECORD: u32 = u32::MAX;

impl Default for CellState {
    fn default() -> Self {
        CellState {
            owner: None,
            troops: 0,
            building: None,
            built_tick: 0,
            progress: 0,
            links: [None; 3],
            link_ready_tick: 0,
            record: NO_RECORD,
        }
    }
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
    pub records: Vec<stats::BuildRecord>,
    pub rng: rng::Rng,
    /// Seeded permutation of seats used by rotating initiative.
    pub seat_order: Vec<PlayerId>,
    pub elim_place: Vec<Option<(u16, u16)>>,
}

impl State {
    pub fn new(settings: MatchSettings, names: Vec<String>) -> Result<State, String> {
        let map = generate(&settings)?;
        let board = Board::new(map.radius);
        let mut cells = vec![CellState::default(); board.slots()];
        for (i, c) in cells.iter_mut().enumerate() {
            c.troops = map.garrison[i];
        }
        for (p, &sp) in map.spawns.iter().enumerate() {
            let c = &mut cells[board.index(sp).unwrap()];
            c.owner = Some(p as PlayerId);
            c.troops = settings.rules.starting_troops;
        }
        let n = settings.players as usize;
        let mut names = names;
        names.resize(n, String::new());
        for (i, nm) in names.iter_mut().enumerate() {
            if nm.is_empty() {
                *nm = format!("Player {}", i + 1);
            }
        }
        let start = settings.rules.start_resource;
        let settings_match_seed = settings.match_seed;
        let mut seat_order: Vec<PlayerId> = (0..n as PlayerId).collect();
        rng::Rng::new(settings_match_seed ^ 0x5EA7).shuffle(&mut seat_order);
        Ok(State {
            settings,
            board,
            terrain: map.terrain,
            cells,
            names,
            resources: vec![start; n],
            eliminated: vec![false; n],
            eliminated_at: vec![None; n],
            tick: 0,
            last_events: vec![],
            result: None,
            records: vec![],
            rng: rng::Rng::new(settings_match_seed),
            seat_order,
            elim_place: vec![None; n],
        })
    }

    pub fn rules(&self) -> &RulesConfig {
        &self.settings.rules
    }

    pub fn cell(&self, h: Hex) -> Option<&CellState> {
        self.board.index(h).map(|i| &self.cells[i])
    }

    fn cell_mut(&mut self, h: Hex) -> &mut CellState {
        let i = self.board.index(h).unwrap();
        &mut self.cells[i]
    }

    pub fn height(&self, h: Hex) -> i8 {
        self.board
            .index(h)
            .and_then(|i| self.terrain[i])
            .map(|t| t.height)
            .unwrap_or(0)
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

    fn is_building(&self, h: Hex, p: PlayerId, b: Building) -> bool {
        self.cell(h)
            .is_some_and(|c| c.owner == Some(p) && c.building == Some(b))
    }

    /// The friendly Farm currently supporting this Village, if any.
    pub fn supporting_farm(&self, village: Hex) -> Option<Hex> {
        let c = self.cell(village)?;
        let owner = c.owner?;
        if c.building != Some(Building::Village) {
            return None;
        }
        village.neighbors().into_iter().find(|&f| {
            self.is_building(f, owner, Building::Farm)
                && self
                    .cell(f)
                    .unwrap()
                    .links
                    .iter()
                    .any(|l| *l == Some(village))
        })
    }

    pub fn wall_links(&self, h: Hex) -> usize {
        self.cell(h)
            .map(|c| c.links.iter().flatten().count())
            .unwrap_or(0)
    }

    /// Defender multiplier (milli) at `h` against an attacker standing at `attacker_height`.
    pub fn defense_milli(&self, h: Hex, attacker_height: i8) -> u32 {
        let wall = self
            .cell(h)
            .filter(|c| c.building == Some(Building::Wall))
            .map(|_| self.wall_links(h));
        self.rules()
            .defense_milli(attacker_height, self.height(h), wall)
    }

    pub fn income(&self, h: Hex) -> u32 {
        let Some(c) = self.cell(h) else { return 0 };
        if c.owner.is_none() {
            return 0;
        }
        let r = self.rules();
        let mut v = r.base_income;
        if c.building == Some(Building::Village) && c.built_tick < self.tick {
            v += r.village.bonus;
            if self.supporting_farm(h).is_some() {
                v += r.village.farm_bonus;
            }
        }
        v
    }

    pub fn near_friendly_quarry(&self, h: Hex, p: PlayerId) -> bool {
        let r = self.rules().quarry.discount_radius;
        self.board
            .hexes_within(h, r)
            .into_iter()
            .any(|n| n != h && self.is_building(n, p, Building::Quarry))
    }

    /// Cost the player would pay to build `b` on `h` right now.
    pub fn cost_for(&self, p: PlayerId, h: Hex, b: Building) -> BuildCost {
        self.rules().build_cost(b, self.near_friendly_quarry(h, p))
    }

    fn can_build(&self, p: PlayerId, at: Hex, b: Building) -> Result<BuildCost, &'static str> {
        let c = self.cell(at).ok_or("hex off board")?;
        if c.owner != Some(p) {
            return Err("hex not owned");
        }
        if c.building.is_some() {
            return Err("hex already has a building");
        }
        if !self.rules().terrain_allowed(b, self.height(at)) {
            return Err("building not allowed at this height");
        }
        if b == Building::Quarry {
            let min = self.rules().quarry.min_friendly_distance;
            let too_close = self
                .board
                .hexes_within(at, min - 1)
                .into_iter()
                .any(|n| self.is_building(n, p, Building::Quarry));
            if too_close {
                return Err("too close to a friendly Quarry");
            }
        }
        let cost = self.cost_for(p, at, b);
        if self.resources[p as usize] < cost.resource {
            return Err("insufficient resource");
        }
        if c.troops < cost.troops + 1 {
            return Err("not enough troops on hex (must leave 1)");
        }
        Ok(cost)
    }

    fn validate_action(&self, p: PlayerId, a: &Action) -> Result<(), &'static str> {
        match a {
            Action::NoOp => Ok(()),
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
            Action::Gather { target } => {
                if self.cell(*target).is_some_and(|c| c.owner == Some(p)) {
                    Ok(())
                } else {
                    Err("gather target not owned")
                }
            }
            Action::Build { at, building } => self.can_build(p, *at, *building).map(|_| ()),
        }
    }

    fn validate_management(&self, p: PlayerId, m: &ManagementCommand) -> Result<(), &'static str> {
        match m {
            ManagementCommand::SetFarmLink {
                farm,
                village,
                enabled,
            } => {
                if !self.is_building(*farm, p, Building::Farm) {
                    return Err("not your Farm");
                }
                let f = self.cell(*farm).unwrap();
                if self.tick < f.link_ready_tick {
                    return Err("link cooldown");
                }
                if *enabled {
                    if farm.distance(*village) != 1 {
                        return Err("Village must be adjacent");
                    }
                    if !self.is_building(*village, p, Building::Village) {
                        return Err("not your Village");
                    }
                    if self.supporting_farm(*village).is_some() {
                        return Err("Village already supported");
                    }
                    let used = f.links.iter().flatten().count();
                    if used >= self.rules().farm.sockets as usize {
                        return Err("no free socket");
                    }
                    Ok(())
                } else if f.links.contains(&Some(*village)) {
                    Ok(())
                } else {
                    Err("not linked")
                }
            }
            ManagementCommand::SetWallLink {
                wall_a: a,
                wall_b: b,
                enabled,
            } => {
                if !self.is_building(*a, p, Building::Wall)
                    || !self.is_building(*b, p, Building::Wall)
                {
                    return Err("both hexes must be your Walls");
                }
                if a.distance(*b) != 1 {
                    return Err("Walls must be adjacent");
                }
                let (ca, cb) = (self.cell(*a).unwrap(), self.cell(*b).unwrap());
                if self.tick < ca.link_ready_tick || self.tick < cb.link_ready_tick {
                    return Err("link cooldown");
                }
                let linked = ca.links.contains(&Some(*b));
                if *enabled {
                    let max = self.rules().wall.max_links as usize;
                    if linked {
                        return Err("already linked");
                    }
                    if self.wall_links(*a) >= max || self.wall_links(*b) >= max {
                        return Err("Wall already has maximum links");
                    }
                } else if !linked {
                    return Err("not linked");
                }
                Ok(())
            }
        }
    }

    fn set_slot(links: &mut [Option<Hex>; 3], idx: Option<usize>, v: Option<Hex>) {
        let i = idx.unwrap_or_else(|| links.iter().position(|l| l.is_none()).unwrap());
        links[i] = v;
    }

    fn apply_management(&mut self, p: PlayerId, m: &ManagementCommand, ev: &mut Vec<Event>) {
        let tick = self.tick;
        match m {
            ManagementCommand::SetFarmLink {
                farm,
                village,
                enabled,
            } => {
                let cd = self.rules().farm.relink_cooldown;
                let c = self.cell_mut(*farm);
                if *enabled {
                    Self::set_slot(&mut c.links, None, Some(*village));
                } else if let Some(i) = c.links.iter().position(|l| *l == Some(*village)) {
                    c.links[i] = None;
                }
                c.link_ready_tick = tick + cd;
                ev.push(Event::LinkChanged {
                    player: p,
                    at: *farm,
                });
            }
            ManagementCommand::SetWallLink {
                wall_a: a,
                wall_b: b,
                enabled,
            } => {
                let cd = self.rules().wall.relink_cooldown;
                for (x, y) in [(*a, *b), (*b, *a)] {
                    let c = self.cell_mut(x);
                    if *enabled {
                        Self::set_slot(&mut c.links, None, Some(y));
                    } else if let Some(i) = c.links.iter().position(|l| *l == Some(y)) {
                        c.links[i] = None;
                    }
                    c.link_ready_tick = tick + cd;
                }
                ev.push(Event::LinkChanged { player: p, at: *a });
            }
        }
    }

    fn apply_build(&mut self, p: PlayerId, at: Hex, b: Building, ev: &mut Vec<Event>) {
        let cost = self.cost_for(p, at, b);
        self.resources[p as usize] -= cost.resource;
        let tick = self.tick;
        let height = self.height(at);
        let n = self.settings.players as usize;
        let rec = self.records.len() as u32;
        self.records.push(stats::BuildRecord {
            kind: b,
            builder: p,
            tick,
            hex: at,
            height,
            troops_paid: cost.troops,
            resource_paid: cost.resource,
            troop_discount: cost.base_troops - cost.troops,
            resource_discount: cost.base_resource - cost.resource,
            owned_ticks: vec![0; n],
            captures: vec![],
            resource_generated: 0,
            village_bonus_troops: 0,
            linked_ticks: 0,
            unlinked_ticks: 0,
            wall_mult_x100_sum: 0,
            wall_mult_ticks: 0,
            wall_fights: 0,
            repaid_tick: None,
        });
        let c = self.cell_mut(at);
        c.troops -= cost.troops;
        c.building = Some(b);
        c.built_tick = tick;
        c.progress = 0;
        c.links = [None; 3];
        c.link_ready_tick = 0;
        c.record = rec;
        ev.push(Event::Built {
            player: p,
            at,
            building: b,
        });
    }

    /// Drop links that are no longer valid around a hex whose ownership changed.
    fn sanitize_around(&mut self, h: Hex, ev: &mut Vec<Event>) {
        let mut around = vec![h];
        around.extend(h.neighbors());
        for x in around {
            let Some(c) = self.cell(x).copied() else {
                continue;
            };
            let Some(owner) = c.owner else { continue };
            let mut links = c.links;
            for l in links.iter_mut() {
                let Some(t) = *l else { continue };
                let tc = self.cell(t);
                let valid = match c.building {
                    Some(Building::Farm) => self.is_building(t, owner, Building::Village),
                    Some(Building::Wall) => {
                        self.is_building(t, owner, Building::Wall)
                            && tc.is_some_and(|t| t.links.iter().any(|b| *b == Some(x)))
                    }
                    _ => false,
                };
                if !valid {
                    *l = None;
                }
            }
            if links != c.links {
                self.cell_mut(x).links = links;
                ev.push(Event::LinksBroken { at: x });
            }
        }
        // A Wall partner may still point at a hex that just dropped it; resolve mutuality.
        for x in h.neighbors().into_iter().chain([h]) {
            let Some(c) = self.cell(x).copied() else {
                continue;
            };
            if c.building != Some(Building::Wall) {
                continue;
            }
            let mut links = c.links;
            for l in links.iter_mut() {
                if let Some(t) = *l {
                    if !self.cell(t).is_some_and(|tc| tc.links.contains(&Some(x))) {
                        *l = None;
                    }
                }
            }
            if links != c.links {
                self.cell_mut(x).links = links;
                ev.push(Event::LinksBroken { at: x });
            }
        }
    }

    /// Deterministic exchange loop from the rules reference, in fixed-point.
    /// Returns (surviving attackers, surviving defenders in milli-troops).
    fn fight(&mut self, attackers: u32, defenders: u32, defense_milli: u32) -> (u32, u64) {
        let win = self.rules().attacker_exchange_win_milli as u64;
        let d = defense_milli.max(1) as u64;
        let per_win = (1_000_000 + d / 2) / d;
        let (mut a, mut def) = (attackers as u64, defenders as u64 * 1000);
        while a > 0 && def > 0 {
            if self.rng.next_u64() % 1000 < win {
                def = def.saturating_sub(per_win);
            } else {
                a -= 1;
            }
        }
        (a as u32, def)
    }

    fn apply_move(&mut self, p: PlayerId, from: Hex, to: Hex, n: u32, ev: &mut Vec<Event>) {
        self.cell_mut(from).troops -= n;
        ev.push(Event::Moved {
            player: p,
            from,
            to,
            troops: n,
        });
        let target = *self.cell(to).unwrap();
        if target.owner == Some(p) {
            self.cell_mut(to).troops += n;
            return;
        }
        let defense = self.defense_milli(to, self.height(from));
        if target.building == Some(Building::Wall) && target.record != NO_RECORD {
            self.records[target.record as usize].wall_fights += 1;
        }
        let (left, def_milli) = self.fight(n, target.troops, defense);
        if def_milli == 0 {
            let tick = self.tick;
            let c = self.cell_mut(to);
            c.owner = Some(p);
            c.troops = left;
            if target.record != NO_RECORD {
                self.records[target.record as usize]
                    .captures
                    .push((tick, p));
            }
            ev.push(Event::Captured {
                player: p,
                at: to,
                previous_owner: target.owner,
                troops_left: left,
                building: target.building,
            });
            self.sanitize_around(to, ev);
            self.resolve_encirclement(p, to, ev);
        } else {
            self.cell_mut(to).troops = ((def_milli + 999) / 1000) as u32;
            ev.push(Event::Repelled {
                attacker: p,
                at: to,
                attackers_lost: n,
            });
        }
    }

    /// Connected component of `owner`-owned hexes containing `start`.
    fn component(&self, start: Hex, owner: PlayerId) -> Vec<Hex> {
        let mut seen = std::collections::BTreeSet::from([start]);
        let mut stack = vec![start];
        while let Some(c) = stack.pop() {
            for nb in c.neighbors() {
                if self.cell(nb).is_some_and(|x| x.owner == Some(owner)) && seen.insert(nb) {
                    stack.push(nb);
                }
            }
        }
        seen.into_iter().collect()
    }

    pub fn is_enclosed(&self, component: &[Hex], capturer: PlayerId) -> bool {
        let rules = self.rules();
        for &h in component {
            for nb in h.neighbors() {
                if !self.board.contains(nb) {
                    return false;
                }
                if !passable(&self.board, &self.terrain, nb) {
                    if rules.impassable_breaks_encirclement {
                        return false;
                    }
                    continue;
                }
                if component.contains(&nb) {
                    continue;
                }
                if self.cell(nb).unwrap().owner != Some(capturer) {
                    return false;
                }
            }
        }
        true
    }

    fn resolve_encirclement(&mut self, capturer: PlayerId, at: Hex, ev: &mut Vec<Event>) {
        if !self.rules().encirclement_enabled {
            return;
        }
        let mut done: Vec<Hex> = vec![];
        for nb in at.neighbors() {
            let Some(owner) = self.cell(nb).and_then(|c| c.owner) else {
                continue;
            };
            if owner == capturer || done.contains(&nb) {
                continue;
            }
            let comp = self.component(nb, owner);
            done.extend(comp.iter().copied());
            if self.is_enclosed(&comp, capturer) {
                for &h in &comp {
                    self.cell_mut(h).owner = Some(capturer);
                }
                for &h in &comp {
                    self.sanitize_around(h, ev);
                }
                ev.push(Event::Surrendered {
                    capturer,
                    from: owner,
                    hexes: comp,
                });
            }
        }
    }

    /// Deterministic one-step-toward-target map over friendly land. Ties use the fixed
    /// axial direction order of `HEX_DIRECTIONS`.
    fn gather(&mut self, p: PlayerId, target: Hex, ev: &mut Vec<Event>) {
        let mut dist = std::collections::BTreeMap::from([(target, 0u32)]);
        let mut q = std::collections::VecDeque::from([target]);
        while let Some(c) = q.pop_front() {
            for nb in c.neighbors() {
                if self.cell(nb).is_some_and(|x| x.owner == Some(p)) && !dist.contains_key(&nb) {
                    dist.insert(nb, dist[&c] + 1);
                    q.push_back(nb);
                }
            }
        }
        let mut transfers: Vec<(Hex, Hex, u32)> = vec![];
        for (&h, &d) in &dist {
            let troops = self.cell(h).unwrap().troops;
            if h == target || troops <= 1 {
                continue;
            }
            if let Some(next) = h
                .neighbors()
                .into_iter()
                .find(|n| dist.get(n) == Some(&(d - 1)))
            {
                transfers.push((h, next, troops - 1));
            }
        }
        let sources = transfers.len() as u32;
        for (from, to, n) in transfers {
            self.cell_mut(from).troops -= n;
            self.cell_mut(to).troops += n;
        }
        ev.push(Event::Gathered {
            player: p,
            target,
            sources,
        });
    }

    fn produce(&mut self) {
        let tick = self.tick;
        let hexes = self.board.hexes();
        let (period, output) = (self.rules().extractor_period, self.rules().extractor_output);
        for h in hexes {
            let c = *self.cell(h).unwrap();
            let Some(owner) = c.owner else { continue };
            let income = self.income(h);
            let active = c.built_tick < tick;
            let supported =
                c.building == Some(Building::Village) && self.supporting_farm(h).is_some();
            let mut gained = 0;
            let mut progress = c.progress;
            if matches!(c.building, Some(Building::Farm | Building::Quarry)) && active {
                progress += 1;
                if progress >= period {
                    progress = 0;
                    gained = output;
                }
            }
            let wall_mult = (c.building == Some(Building::Wall)).then(|| self.wall_only_milli(h));
            let linked = match c.building {
                Some(Building::Farm | Building::Wall) => c.links.iter().any(|l| l.is_some()),
                Some(Building::Village) => supported,
                _ => false,
            };
            {
                let cm = self.cell_mut(h);
                cm.troops += income;
                cm.progress = progress;
            }
            self.resources[owner as usize] += gained;
            if c.record != NO_RECORD {
                let troop_cost = self.records[c.record as usize].troops_paid;
                let r = &mut self.records[c.record as usize];
                r.owned_ticks[owner as usize] += 1;
                r.resource_generated += gained;
                if c.building == Some(Building::Village) && active {
                    r.village_bonus_troops += income - self.settings.rules.base_income;
                    if r.repaid_tick.is_none() && r.village_bonus_troops >= troop_cost {
                        r.repaid_tick = Some(tick + 1);
                    }
                }
                if c.building != Some(Building::Quarry) {
                    if linked {
                        r.linked_ticks += 1;
                    } else {
                        r.unlinked_ticks += 1;
                    }
                }
                if let Some(m) = wall_mult {
                    r.wall_mult_x100_sum += m as u64 / 10;
                    r.wall_mult_ticks += 1;
                }
            }
        }
    }

    fn wall_only_milli(&self, h: Hex) -> u32 {
        self.rules().wall_multiplier_milli(self.wall_links(h))
    }

    /// Alive seats, rotated by `(match_seed + tick) % alive`. Independent of input arrival order.
    pub fn initiative_order(&self) -> Vec<PlayerId> {
        let mut alive: Vec<PlayerId> = self
            .seat_order
            .iter()
            .copied()
            .filter(|&p| !self.eliminated[p as usize])
            .collect();
        let first = (self.settings.match_seed as usize + self.tick as usize) % alive.len().max(1);
        alive.rotate_left(first);
        alive
    }

    fn mark_eliminations(&mut self, events: &mut Vec<Event>) {
        let n = self.settings.players as usize;
        let counts = self.hex_counts();
        let fresh: Vec<usize> = (0..n)
            .filter(|&p| !self.eliminated[p] && counts[p] == 0)
            .collect();
        if fresh.is_empty() {
            return;
        }
        let alive_after = (0..n).filter(|&p| !self.eliminated[p]).count() - fresh.len();
        for &p in &fresh {
            self.eliminated[p] = true;
            self.eliminated_at[p] = Some(self.tick);
            self.elim_place[p] = Some((alive_after as u16 + 1, (alive_after + fresh.len()) as u16));
            events.push(Event::Eliminated {
                player: p as PlayerId,
            });
        }
    }

    /// Advance one authoritative tick. Missing commands are NoOp.
    pub fn step(&mut self, commands: &[(PlayerId, PlayerCommand)]) {
        if self.result.is_some() {
            return;
        }
        let mut events = vec![];
        for p in self.initiative_order() {
            if self.eliminated[p as usize] {
                continue;
            }
            let Some((_, cmd)) = commands.iter().find(|(pid, _)| *pid == p) else {
                continue;
            };
            for m in &cmd.management {
                match self.validate_management(p, m) {
                    Ok(()) => self.apply_management(p, m, &mut events),
                    Err(reason) => events.push(Event::Rejected {
                        player: p,
                        reason: reason.into(),
                    }),
                }
            }
            match self.validate_action(p, &cmd.action) {
                Err(reason) => events.push(Event::Rejected {
                    player: p,
                    reason: reason.into(),
                }),
                Ok(()) => match &cmd.action {
                    Action::NoOp => {}
                    Action::Move { from, to, troops } => {
                        self.apply_move(p, *from, *to, *troops, &mut events)
                    }
                    Action::Gather { target } => self.gather(p, *target, &mut events),
                    Action::Build { at, building } => {
                        self.apply_build(p, *at, *building, &mut events)
                    }
                },
            }
            self.mark_eliminations(&mut events);
        }
        self.produce();
        self.mark_eliminations(&mut events);
        self.tick += 1;
        self.last_events = events;
        let alive = (0..self.settings.players as usize)
            .filter(|&p| !self.eliminated[p])
            .count();
        if alive <= 1 || self.tick >= self.rules().max_ticks {
            self.result = Some(self.final_result());
        }
    }

    /// Final standings: elimination order, then land, troops, resource, buildings.
    pub fn final_result(&self) -> MatchResult {
        let n = self.settings.players as usize;
        let counts = self.hex_counts();
        let mut troops = vec![0u64; n];
        let mut buildings = vec![0u32; n];
        for c in &self.cells {
            if let Some(o) = c.owner {
                troops[o as usize] += c.troops as u64;
                buildings[o as usize] += c.building.is_some() as u32;
            }
        }
        let keys: Vec<scoring::SurvivorKey> = (0..n)
            .map(|p| (counts[p], troops[p], self.resources[p], buildings[p]))
            .collect();
        scoring::build_result(
            &self.rules().version,
            self.tick,
            &self.eliminated_at,
            &keys,
            &counts,
            &troops,
            &self.resources,
            &buildings,
        )
    }

    pub fn building_report(&self) -> stats::BuildingReport {
        let result = self.result.clone().unwrap_or_else(|| self.final_result());
        let mut r = stats::BuildingReport::default();
        r.add_match(&self.records, &result);
        r.finalize();
        r
    }

    /// Hexes the player can currently see (bitmap over board slots).
    pub fn visible_mask(&self, player: PlayerId) -> Vec<bool> {
        let r = self.rules().fog_radius;
        let mut mask = vec![false; self.board.slots()];
        for h in self.board.hexes() {
            if self.cell(h).unwrap().owner != Some(player) {
                continue;
            }
            for n in self.board.hexes_within(h, r) {
                mask[self.board.index(n).unwrap()] = true;
            }
        }
        mask
    }

    fn visible_cell(&self, h: Hex) -> VisibleHex {
        let c = self.cell(h).unwrap();
        let r = self.rules();
        let building = c.building.map(|kind| {
            let links: Vec<Hex> = match kind {
                Building::Village => self.supporting_farm(h).into_iter().collect(),
                _ => c.links.iter().flatten().copied().collect(),
            };
            let production_remaining = match kind {
                Building::Farm | Building::Quarry => r.extractor_period.saturating_sub(c.progress),
                _ => 0,
            };
            VisibleBuilding {
                kind,
                production_remaining,
                links,
                link_cooldown: c.link_ready_tick.saturating_sub(self.tick),
            }
        });
        let wall_milli = if c.building == Some(Building::Wall) {
            self.wall_only_milli(h)
        } else {
            MILLI
        };
        VisibleHex {
            hex: h,
            owner: c.owner,
            troops: c.troops,
            building,
            income: self.income(h),
            wall_milli,
        }
    }

    /// The only information channel to a player. Depends solely on public data, the
    /// player's own state, and hexes within the player's vision.
    pub fn view(&self, player: PlayerId) -> View {
        let mask = self.visible_mask(player);
        let vis = |h: Hex| self.board.index(h).map(|i| mask[i]).unwrap_or(false);
        let public = self.rules().public_terrain;
        let public_terrain = self
            .terrain
            .iter()
            .enumerate()
            .map(|(i, t)| if public || mask[i] { *t } else { None })
            .collect();
        let visible_hexes = self
            .board
            .hexes()
            .into_iter()
            .filter(|&h| vis(h))
            .map(|h| self.visible_cell(h))
            .collect();
        let visible_events = self
            .last_events
            .iter()
            .filter(|e| match e {
                Event::Moved { from, to, .. } => vis(*from) || vis(*to),
                Event::Gathered {
                    player: p, target, ..
                } => *p == player || vis(*target),
                Event::Captured { at, .. }
                | Event::Repelled { at, .. }
                | Event::Built { at, .. }
                | Event::LinkChanged { at, .. }
                | Event::LinksBroken { at } => vis(*at),
                Event::Surrendered { hexes, .. } => hexes.iter().any(|h| vis(*h)),
                Event::Eliminated { .. } => true,
                Event::Rejected { player: p, .. } => *p == player,
            })
            .cloned()
            .collect();
        View {
            protocol_version: PROTOCOL_VERSION,
            rules_version: self.rules().version.clone(),
            tick: self.tick,
            you: player,
            radius: self.board.radius,
            fog_radius: self.rules().fog_radius,
            public_players: (0..self.settings.players)
                .map(|id| PublicPlayer {
                    id,
                    name: self.names[id as usize].clone(),
                    alive: !self.eliminated[id as usize],
                    placement: self.elim_place[id as usize],
                })
                .collect(),
            public_terrain,
            visible_hexes,
            own_economy: PlayerEconomy {
                resource: self.resources[player as usize],
            },
            own_queue: vec![],
            visible_events,
        }
    }

    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |v: u64| {
            h ^= v;
            h = h.wrapping_mul(0x100_0000_01b3);
        };
        feed(self.tick as u64);
        feed(self.rng.clone().next_u64());
        for c in &self.cells {
            feed(c.owner.map(|o| o as u64 + 1).unwrap_or(0));
            feed(c.troops as u64);
            feed(c.building.map(|b| b as u64 + 1).unwrap_or(0));
            feed(c.progress as u64);
            for l in c.links.iter().flatten() {
                feed((l.q as i64 * 1000 + l.r as i64) as u64);
            }
        }
        for r in &self.resources {
            feed(*r as u64);
        }
        h
    }
}

/// Package a finished match into a replay.
pub fn build_replay(state: &State, commands: Vec<TickCommands>, events: Vec<Vec<Event>>) -> Replay {
    let rules = state.rules().clone();
    Replay {
        protocol_version: PROTOCOL_VERSION,
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        rules_version: rules.version.clone(),
        rules_hash: rules.hash(),
        terrain_seed: state.settings.terrain_seed,
        spawn_seed: state.settings.spawn_seed,
        match_seed: state.settings.match_seed,
        players: state
            .names
            .iter()
            .enumerate()
            .map(|(i, n)| ReplayPlayer {
                id: i as PlayerId,
                name: n.clone(),
            })
            .collect(),
        commands,
        events,
        result: state.result.clone().unwrap_or_else(|| state.final_result()),
        checksum: format!("{:016x}", state.state_hash()),
        rules,
    }
}

/// Re-simulate a replay from its seeds and recorded commands.
pub fn replay_state(replay: &Replay) -> Result<State, String> {
    if replay.rules.hash() != replay.rules_hash {
        return Err("rules hash mismatch".into());
    }
    let names = replay.players.iter().map(|p| p.name.clone()).collect();
    let mut st = State::new(replay.settings(), names)?;
    for tick in &replay.commands {
        st.step(tick);
    }
    Ok(st)
}

#[cfg(test)]
mod tests;
