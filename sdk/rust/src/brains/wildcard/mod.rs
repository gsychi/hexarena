//! Mason: Pangolin's Farm-Village lattice and Ridgeline's border play, rebuilt for rules
//! v0.7, where a capture that seals a region off from the board edge takes all of it.
//!
//! The capture core ([`core`]) prices every hex by the exact regions taking it would
//! seal (one Tarjan pass over the land we don't own, rooted at the board edge, in
//! [`rings`]), credits first steps of two-step rings, launches or stages for the big
//! ones, and garrisons the necks through which a rival could swallow a pocket of ours.
//! This layer never lets a build or a Wall take the tick from a ring capture, and when a
//! rival is about to take a Village of ours (directly, or by sealing the pocket it sits
//! in) it demolishes the Village first: captures transfer buildings, demolishing
//! doesn't refund, and a lattice Village is worth up to 8 troops a tick to whoever owns
//! it.
//!
//! Lattice and Walls below are Pangolin's (agent:terrain), unchanged:
//!
//! Colour every hex by `(q - r) mod 3`: no two neighbours share a colour, and each hex
//! has three neighbours of each other colour. Villages go on one colour and Farms on the
//! other two, so in a finished patch every Village touches six Farms (8 troops a tick)
//! and every Farm boosts three Villages. The colour is picked once, from the land around
//! our start, to put Villages on hills (no Farm can stand there) and Farms on low ground.
//! Only hexes at least `depth` steps behind our border are built on, and each build is
//! ranked by the troops per tick it adds per troop it costs, counting the lattice
//! neighbours it will feed later at `future_pct`. A Quarry goes on a hill whose
//! neighbours are mostly unbuilt sites, when its discount pays for it.
//!
//! The tick goes to a build instead of Core's move when that move isn't a capture,
//! or when the build pays back faster than `rush_roi`.

pub mod core;
pub mod hold;
pub mod net;
pub mod rings;

use self::core::Core;
use self::rings::{Board, Tarjan};
use crate::{Action, Bot, Building, Hex, ManagementCommand, PlayerCommand, PlayerId, RulesConfig, Style, View};
use std::collections::VecDeque;

struct Map {
    radius: i32,
    width: i32,
    passable: Vec<bool>,
    height: Vec<i8>,
    hexes: Vec<Hex>,
    nbrs: Vec<Vec<usize>>,
}

impl Map {
    fn new(view: &View) -> Map {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let mut passable = vec![false; slots];
        let mut height = vec![0i8; slots];
        let hexes: Vec<Hex> =
            (0..slots).map(|i| Hex::new(i as i32 % width - radius, i as i32 / width - radius)).collect();
        for i in 0..slots {
            if let Some(t) = view.public_terrain[i] {
                passable[i] = t.passable();
                height[i] = t.height;
            }
        }
        let mut m = Map { radius, width, passable, height, hexes, nbrs: vec![] };
        m.nbrs = (0..slots)
            .map(|i| {
                m.hexes[i].neighbors().iter().filter_map(|n| m.slot(*n)).filter(|&j| m.passable[j]).collect()
            })
            .collect();
        m
    }

    fn slot(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * self.width + h.q + r) as usize)
    }

    fn colour(&self, i: usize) -> i32 {
        let h = self.hexes[i];
        (h.q - h.r).rem_euclid(3)
    }
}

struct Knobs {
    /// No builds before this tick.
    build_from: u32,
    /// No builds from this tick on (0 = build all game).
    build_until: u32,
    /// Fewest ticks between two builds.
    build_gap: u32,
    /// Fewest steps from a build site to land we don't own.
    depth: u32,
    /// % of a troop a tick credited for each empty lattice partner a build will feed.
    future_pct: u64,
    /// Builds paying back less than this (troops a tick per 1000 troops) are skipped.
    min_roi: u64,
    /// Builds paying back at least this take the tick even from a capture.
    rush_roi: u64,
    /// Quarries on hills (0/1).
    quarry: bool,
    /// Lattice sites (x10 troops saved per troop paid) a Quarry must discount to be built.
    quarry_min_x10: u64,
    /// Troops a tick a Quarry adds to each adjacent Village (the frozen binary can't read
    /// it from newer live rules, so it is a knob).
    quarry_village: u64,
    /// Walls on enemy-facing border hexes from this tick (0 = never).
    wall_from: u32,
    /// Fewest ticks between two Walls.
    wall_gap: u32,
    /// Most Walls to own.
    wall_cap: usize,
    /// Troops a border hex keeps on top of a Wall's cost before it may build one.
    wall_keep: u32,
    /// Turtle triggers (bits, 0 = off): 1 = an exposed start (central and open, read from
    /// public terrain at tick 0), 2 = an enemy hex seen near the start early. A turtle
    /// walls its enemy-facing border from `turtle_wall_from` until `wall_from`.
    turtle: u32,
    /// Fewest steps from the start to the board edge that count as central.
    turtle_edge: i32,
    /// Fewest passable hexes within 6 steps of the start that count as open.
    turtle_open: usize,
    /// Contact counts before this tick, within `turtle_dist` of the start.
    turtle_tick: u32,
    turtle_dist: i32,
    turtle_wall_from: u32,
    turtle_cap: usize,
    turtle_gap: u32,
    /// Demolish Villages a rival is about to take (0/1).
    demolish: bool,
    /// Least income (troops a tick) a Village must make to be worth a demolish.
    demolish_income: u32,
    /// A hex counts as about to fall when the rival stack next to it kills at least
    /// this % of its defenders going all in.
    demolish_pct: u64,
    /// Fewest ticks between two demolitions.
    demolish_gap: u32,
    /// Ticks a demolished site stays off the build list.
    demolish_ban: u32,
    /// Most defenders a Village hex may hold and still count as about to fall.
    demolish_def: u32,
    /// Demolish Walls deep inside our land, on ticks no capture wants (0/1).
    clear_walls: bool,
    /// Demolish a captured building off our lattice colouring when the lattice building
    /// would add at least this many troops a tick (0 = never).
    lattice_fix: u64,
}

impl Knobs {
    fn read(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Knobs {
            build_from: k("build_from", 200) as u32,
            build_until: k("build_until", 0) as u32,
            build_gap: k("build_gap", 2) as u32,
            depth: k("depth", 2) as u32,
            future_pct: k("future_pct", 50) as u64,
            min_roi: k("min_roi", 10) as u64,
            rush_roi: k("rush_roi", 60) as u64,
            quarry: k("quarry", 0) > 0,
            quarry_min_x10: k("quarry_min_x10", 20) as u64,
            quarry_village: k("quarry_village", 0) as u64,
            wall_from: k("wall_from", 0) as u32,
            wall_gap: k("wall_gap", 4) as u32,
            wall_cap: k("wall_cap", 60) as usize,
            wall_keep: k("wall_keep", 10) as u32,
            turtle: k("turtle", 0) as u32,
            turtle_edge: k("turtle_edge", 5) as i32,
            turtle_open: k("turtle_open", 80) as usize,
            turtle_tick: k("turtle_tick", 150) as u32,
            turtle_dist: k("turtle_dist", 6) as i32,
            turtle_wall_from: k("turtle_wall_from", 150).max(1) as u32,
            turtle_cap: k("turtle_cap", 20) as usize,
            turtle_gap: k("turtle_gap", 6) as u32,
            demolish: k("demolish", 0) > 0,
            demolish_income: k("demolish_income", 3) as u32,
            demolish_pct: k("demolish_pct", 2600) as u64,
            demolish_gap: k("demolish_gap", 4) as u32,
            demolish_ban: k("demolish_ban", 300) as u32,
            demolish_def: k("demolish_def", 40) as u32,
            clear_walls: k("clear_walls", 0) > 0,
            lattice_fix: k("lattice_fix", 0) as u64,
        }
    }
}

pub struct Wildcard {
    /// The genome, until the first view shows whether this is a 1v1 or a free-for-all.
    style: Option<Style>,
    /// In a free-for-all, switch to the 1v1 knobs once this few players are alive (0 = never).
    duel_when: usize,
    duel_style: Option<Style>,
    inner: Core,
    rules: RulesConfig,
    k: Knobs,
    map: Option<Map>,
    /// Lattice colour that holds Villages.
    vclass: Option<i32>,
    last_build: Option<u32>,
    last_wall: Option<u32>,
    start: Option<usize>,
    turtling: bool,
    board: Option<Board>,
    tarjan: Tarjan,
    last_demolish: Option<u32>,
    /// Our hexes a rival is about to take, directly or by sealing their pocket.
    danger: Vec<bool>,
    /// Tick until which each hex gets no new building (after a demolish).
    banned: Vec<u32>,
    /// Debug calibration of the danger test: flagged (tick, slot, bucket) and per-bucket
    /// (flagged, lost within 10 ticks).
    calib: Vec<(u32, usize, usize)>,
    calib_hits: [(u32, u32); 16],
    /// `MASON_CALIB` / `MASON_DEBUG` set when the bot was built.
    calib_on: bool,
    debug_on: bool,
    /// Thread CPU time per move, measured when `WILDCARD_CPU` is set: (moves, total ns, worst ns).
    cpu: Option<(u64, u64, u64)>,
}

/// CPU time this thread has used, in nanoseconds.
fn thread_cpu_ns() -> u64 {
    #[repr(C)]
    struct Timespec {
        sec: i64,
        nsec: i64,
    }
    extern "C" {
        fn clock_gettime(clock: i32, tp: *mut Timespec) -> i32;
    }
    const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
    let mut t = Timespec { sec: 0, nsec: 0 };
    // SAFETY: clock_gettime only writes the timespec we pass it.
    unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut t) };
    t.sec as u64 * 1_000_000_000 + t.nsec as u64
}

#[derive(Clone, Copy, PartialEq)]
enum Site {
    Empty,
    Mine(Building),
    Other,
}

impl Wildcard {
    pub fn new(style: Style) -> Wildcard {
        // Every knob a format can override is read here once, so the league sees them all.
        for prefix in ["duel_", "ffa_"] {
            Self::for_format(&style, prefix);
        }
        Wildcard {
            style: Some(style.clone()),
            duel_when: 0,
            duel_style: None,
            inner: Core::new(style.clone()),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            map: None,
            vclass: None,
            last_build: None,
            last_wall: None,
            start: None,
            turtling: false,
            board: None,
            tarjan: Tarjan::default(),
            last_demolish: None,
            danger: vec![],
            banned: vec![],
            calib: vec![],
            calib_hits: [(0, 0); 16],
            calib_on: std::env::var("MASON_CALIB").is_ok(),
            debug_on: std::env::var("MASON_DEBUG").is_ok(),
            cpu: std::env::var("WILDCARD_CPU").is_ok().then_some((0, 0, 0)),
        }
    }

    /// Central and open: the starts that meet the most rivals.
    fn exposed(&self, m: &Map, start: usize) -> bool {
        if m.radius - m.hexes[start].distance(Hex::new(0, 0)) < self.k.turtle_edge {
            return false;
        }
        let mut dist = vec![u32::MAX; m.passable.len()];
        dist[start] = 0;
        let mut queue = VecDeque::from([start]);
        let mut open = 0;
        while let Some(c) = queue.pop_front() {
            open += 1;
            if dist[c] == 6 {
                continue;
            }
            for &n in &m.nbrs[c] {
                if dist[n] == u32::MAX {
                    dist[n] = dist[c] + 1;
                    queue.push_back(n);
                }
            }
        }
        open >= self.k.turtle_open
    }

    fn update_turtle(&mut self, view: &View, m: &Map) {
        let Some(start) = self.start else { return };
        if self.k.turtle & 2 != 0 && view.tick < self.k.turtle_tick {
            let s = m.hexes[start];
            self.turtling |= view.visible_hexes.iter().any(|c| {
                c.owner.is_some_and(|o| o != view.you) && c.hex.distance(s) <= self.k.turtle_dist
            });
        }
    }

    fn farmable(&self, m: &Map, i: usize) -> bool {
        self.rules.terrain_allowed(Building::Farm, m.height[i])
    }

    fn quarry_ok(&self, m: &Map, i: usize) -> bool {
        self.rules.terrain_allowed(Building::Quarry, m.height[i])
    }

    /// The colour whose hexes near `start` make the best Village sites.
    fn pick_class(&self, m: &Map, start: usize) -> i32 {
        let mut best = (i64::MIN, 0);
        for c in 0..3 {
            let mut score = 0i64;
            for i in 0..m.passable.len() {
                if !m.passable[i] || m.colour(i) != c || m.hexes[i].distance(m.hexes[start]) > 6 {
                    continue;
                }
                score += m.nbrs[i].iter().filter(|&&n| self.farmable(m, n)).count() as i64;
                if !self.farmable(m, i) {
                    score += 3;
                }
            }
            if score > best.0 {
                best = (score, c);
            }
        }
        best.1
    }

    fn build(&mut self, view: &View, m: &Map, vclass: i32) -> Option<(u64, Action)> {
        let me = view.you;
        let slots = m.passable.len();
        let mut owner: Vec<Option<PlayerId>> = vec![None; slots];
        let mut site = vec![Site::Other; slots];
        let mut troops = vec![0u32; slots];
        for c in &view.visible_hexes {
            let Some(i) = m.slot(c.hex) else { continue };
            owner[i] = c.owner;
            troops[i] = c.troops;
            if c.owner == Some(me) {
                site[i] = c.building.as_ref().map_or(Site::Empty, |b| Site::Mine(b.kind));
            }
        }
        // Steps from land we don't own, through our own land.
        let mut depth = vec![u32::MAX; slots];
        let mut queue = VecDeque::new();
        for i in 0..slots {
            if m.passable[i] && owner[i] != Some(me) {
                depth[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(c) = queue.pop_front() {
            for &n in &m.nbrs[c] {
                if depth[n] == u32::MAX {
                    depth[n] = depth[c] + 1;
                    queue.push_back(n);
                }
            }
        }
        if self.k.clear_walls {
            // A Wall deep inside our land (usually taken with a sealed pocket) guards
            // nothing and blocks a lattice slot.
            let wall = (0..slots).find(|&i| {
                site[i] == Site::Mine(Building::Wall)
                    && depth[i] != u32::MAX
                    && depth[i] > self.k.depth
                    && troops[i] > self.rules.village.troop_cost
            });
            if let Some(i) = wall {
                return Some((self.k.min_roi, Action::Demolish { at: m.hexes[i] }));
            }
        }
        let near_quarry = |i: usize| m.nbrs[i].iter().any(|&n| site[n] == Site::Mine(Building::Quarry));
        let cost = |b: Building, i: usize| self.rules.build_cost(b, near_quarry(i)).troops;
        if self.k.lattice_fix > 0 {
            // Captured buildings off our lattice colouring: a Farm or Wall on a Village slot
            // feeds nothing, a Wall on a Farm slot feeds nothing. Tear them down when the
            // lattice building would add enough troops a tick; the build pass refills the
            // slot on a later tick.
            let adj = |i: usize, b: Building| m.nbrs[i].iter().filter(|&&n| site[n] == Site::Mine(b)).count() as u64;
            let vb = self.rules.village.bonus as u64;
            let fb = self.rules.village.farm_bonus as u64;
            let mut fix: Option<(u64, usize)> = None;
            for i in 0..slots {
                let Site::Mine(have) = site[i] else { continue };
                if depth[i] == u32::MAX || depth[i] < self.k.depth || troops[i] <= cost(Building::Village, i) {
                    continue;
                }
                let gain = if m.colour(i) == vclass {
                    let village = vb + fb * adj(i, Building::Farm) + self.k.quarry_village * adj(i, Building::Quarry);
                    let now = match have {
                        Building::Village => village,
                        Building::Farm => fb * adj(i, Building::Village),
                        Building::Quarry | Building::Wall => 0,
                    };
                    village.saturating_sub(now)
                } else if have == Building::Wall && self.farmable(m, i) {
                    fb * adj(i, Building::Village)
                } else {
                    0
                };
                if gain >= self.k.lattice_fix && fix.is_none_or(|f| (gain, std::cmp::Reverse(i)) > (f.0, std::cmp::Reverse(f.1))) {
                    fix = Some((gain, i));
                }
            }
            if let Some((gain, i)) = fix {
                let roi = gain * 1000 / (2 * cost(Building::Village, i).max(1) as u64);
                return Some((roi.max(self.k.min_roi), Action::Demolish { at: m.hexes[i] }));
            }
        }
        let fut = self.k.future_pct * 10;
        let mut best: Option<(u64, usize, Building)> = None;
        for i in 0..slots {
            if site[i] != Site::Empty
                || depth[i] < self.k.depth
                || depth[i] == u32::MAX
                || self.danger.get(i).copied().unwrap_or(false)
                || self.banned.get(i).is_some_and(|&t| view.tick < t)
            {
                continue;
            }
            let count = |b: Building| m.nbrs[i].iter().filter(|&&n| site[n] == Site::Mine(b)).count() as u64;
            let mut options: Vec<(Building, u64)> = vec![];
            if m.colour(i) == vclass {
                let partners =
                    m.nbrs[i].iter().filter(|&&n| site[n] == Site::Empty && self.farmable(m, n)).count() as u64;
                let rate = 1000 * (self.rules.village.bonus as u64
                    + self.rules.village.farm_bonus as u64 * count(Building::Farm)
                    + self.k.quarry_village * count(Building::Quarry))
                    + fut * partners;
                options.push((Building::Village, rate));
            } else if self.farmable(m, i) {
                let partners =
                    m.nbrs[i].iter().filter(|&&n| site[n] == Site::Empty && m.colour(n) == vclass).count() as u64;
                let rate = 1000 * self.rules.village.farm_bonus as u64 * count(Building::Village) + fut * partners;
                options.push((Building::Farm, rate));
            } else if self.k.quarry && self.quarry_ok(m, i) {
                let close = (0..slots).any(|j| {
                    site[j] == Site::Mine(Building::Quarry)
                        && m.hexes[j].distance(m.hexes[i]) < self.rules.quarry.min_friendly_distance
                });
                if !close {
                    let saved: u64 = m.nbrs[i]
                        .iter()
                        .filter(|&&n| site[n] == Site::Empty)
                        .map(|&n| {
                            let b = if m.colour(n) == vclass { Building::Village } else { Building::Farm };
                            if b == Building::Farm && !self.farmable(m, n) {
                                return 0;
                            }
                            let c = self.rules.build_cost(b, true);
                            (c.base_troops - c.troops) as u64
                        })
                        .sum();
                    let c = cost(Building::Quarry, i) as u64;
                    let qv = self.k.quarry_village;
                    if qv > 0 {
                        let vsites = m.nbrs[i]
                            .iter()
                            .filter(|&&n| site[n] == Site::Empty && m.colour(n) == vclass)
                            .count() as u64;
                        // A troop saved on a later build counts like a troop a tick for 100 ticks.
                        let rate = 1000 * qv * count(Building::Village) + fut * qv * vsites + saved * 10;
                        options.push((Building::Quarry, rate));
                    } else if saved * 10 >= c * self.k.quarry_min_x10 {
                        options.push((Building::Quarry, self.k.rush_roi * c));
                    }
                }
            }
            for (b, rate) in options {
                let c = cost(b, i);
                if troops[i] < c + 1 {
                    continue;
                }
                let roi = rate / c.max(1) as u64;
                if roi >= self.k.min_roi && best.is_none_or(|x| (roi, std::cmp::Reverse(i)) > (x.0, std::cmp::Reverse(x.1))) {
                    best = Some((roi, i, b));
                }
            }
        }
        best.map(|(roi, i, b)| (roi, Action::Build { at: m.hexes[i], building: b }))
    }

    /// A Wall on the enemy-facing border hex under the biggest adjacent enemy stack,
    /// preferring hexes next to our Walls that still have a free link.
    fn wall(&self, view: &View, m: &Map, cap: usize) -> Option<Action> {
        let me = view.you;
        let slots = m.passable.len();
        let mut cell = vec![None; slots];
        for c in &view.visible_hexes {
            if let Some(i) = m.slot(c.hex) {
                cell[i] = Some(c);
            }
        }
        let is_wall = |c: &crate::VisibleHex| c.building.as_ref().is_some_and(|b| b.kind == Building::Wall);
        let walls = cell.iter().flatten().filter(|c| c.owner == Some(me) && is_wall(c)).count();
        if walls >= cap {
            return None;
        }
        let max_links = self.rules.wall.max_links as usize;
        let mut best: Option<(u64, usize)> = None;
        for i in 0..slots {
            let Some(c) = cell[i] else { continue };
            if c.owner != Some(me) || c.building.is_some() {
                continue;
            }
            let near_quarry = m.nbrs[i].iter().any(|&n| {
                cell[n].is_some_and(|x| x.owner == Some(me) && x.building.as_ref().is_some_and(|b| b.kind == Building::Quarry))
            });
            let cost = self.rules.build_cost(Building::Wall, near_quarry).troops;
            if c.troops < cost + 1 + self.k.wall_keep {
                continue;
            }
            let mut threat = 0u64;
            let mut linkable = 0u64;
            for &n in &m.nbrs[i] {
                let Some(x) = cell[n] else { continue };
                match x.owner {
                    Some(o) if o != me => threat = threat.max(x.troops as u64),
                    Some(_) if is_wall(x) && x.building.as_ref().unwrap().links.len() < max_links => linkable += 1,
                    _ => {}
                }
            }
            if threat == 0 {
                continue;
            }
            let score = linkable * 1_000_000 + threat;
            if best.is_none_or(|b| (score, std::cmp::Reverse(i)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| Action::Build { at: m.hexes[i], building: Building::Wall })
    }

    /// Links between adjacent Walls of ours that both have a free link and no cooldown.
    fn links(&self, view: &View, m: &Map) -> Vec<ManagementCommand> {
        let me = view.you;
        let max_links = self.rules.wall.max_links as usize;
        let mut walls: Vec<(usize, Hex, usize, Vec<Hex>)> = vec![];
        for c in &view.visible_hexes {
            let Some(b) = c.building.as_ref().filter(|b| b.kind == Building::Wall && c.owner == Some(me)) else { continue };
            if b.link_cooldown == 0 {
                walls.push((m.slot(c.hex).unwrap_or(0), c.hex, b.links.len(), b.links.clone()));
            }
        }
        walls.sort_by_key(|w| w.0);
        let mut out = vec![];
        for a in 0..walls.len() {
            for b in a + 1..walls.len() {
                if walls[a].2 >= max_links {
                    break;
                }
                if walls[b].2 >= max_links || walls[a].1.distance(walls[b].1) != 1 || walls[a].3.contains(&walls[b].1) {
                    continue;
                }
                out.push(ManagementCommand::SetWallLink { wall_a: walls[a].1, wall_b: walls[b].1, enabled: true });
                // A link puts both Walls on cooldown, so each takes part in one link a tick.
                walls[a].2 = max_links;
                walls[b].2 = max_links;
            }
        }
        out
    }
}

impl Wildcard {
    /// The best-paying Village of ours a rival is about to take, either by capturing its
    /// hex or by capturing the neck of the pocket it sits in, unless this tick's action
    /// already moves on that hex or neck.
    fn endangered_village(&mut self, view: &View, m: &Map, action: &Action) -> Option<Hex> {
        let me = view.you;
        if self.board.is_none() {
            self.board = Some(Board::new(view));
        }
        let b = self.board.take().unwrap();
        let slots = m.passable.len();
        let mut cell: Vec<Option<&crate::VisibleHex>> = vec![None; slots];
        for c in &view.visible_hexes {
            if let Some(i) = m.slot(c.hex) {
                cell[i] = Some(c);
            }
        }
        let busy = match action {
            Action::Move { to, .. } => m.slot(*to),
            _ => None,
        };
        let p = self.rules.attacker_win_milli(view.tick) as u64;
        let max_def = self.rules.maximum_total_defense_milli as u64;
        // Whether rival `r` has a stack next to `x` that kills `demolish_pct`% of its defenders.
        let falls = |x: usize, r: PlayerId| -> bool {
            let Some(xc) = cell[x] else { return false };
            if xc.owner == Some(me) && xc.troops > self.k.demolish_def {
                return false;
            }
            let def = xc.troops.max(1) as u64;
            m.nbrs[x].iter().any(|&e| {
                cell[e].is_some_and(|ec| {
                    ec.owner == Some(r) && ec.troops >= 2 && {
                        let h = self.rules.height_multiplier_milli(m.height[e], m.height[x]) as u64;
                        let d = (h * xc.wall_milli.max(1000) as u64 / 1000).min(max_def).max(1);
                        let kills = (ec.troops - 1) as u64 * p * 1000 / ((1000 - p) * d);
                        kills * 100 >= def * self.k.demolish_pct
                    }
                })
            })
        };
        let village = |i: usize| {
            cell[i].filter(|c| {
                c.owner == Some(me)
                    && c.income >= self.k.demolish_income
                    && c.building.as_ref().is_some_and(|b| b.kind == Building::Village)
            })
        };
        let mut best: Option<(u32, usize)> = None;
        self.danger.clear();
        self.danger.resize(slots, false);
        let mut danger = std::mem::take(&mut self.danger);
        let mut consider = |i: usize, best: &mut Option<(u32, usize)>| {
            danger[i] = true;
            if let Some(c) = village(i) {
                if Some(i) != busy && best.is_none_or(|b| (c.income, std::cmp::Reverse(i)) > (b.0, std::cmp::Reverse(b.1))) {
                    *best = Some((c.income, i));
                }
            }
        };
        let mut rivals: Vec<PlayerId> = view
            .visible_hexes
            .iter()
            .filter_map(|c| c.owner)
            .filter(|&o| o != me)
            .collect();
        rivals.sort_unstable();
        rivals.dedup();
        let mut blocked = vec![false; slots];
        let mut floods = 0;
        for r in rivals {
            for i in 0..slots {
                blocked[i] = cell[i].is_some_and(|c| c.owner == Some(r));
            }
            for x in 0..slots {
                if blocked[x] || !m.passable[x] || !m.nbrs[x].iter().any(|&n| blocked[n]) || !falls(x, r) {
                    continue;
                }
                if self.calib_on && village(x).is_some() && !self.calib.iter().any(|c| c.1 == x) {
                    let def = cell[x].map_or(1, |c| c.troops.max(1)) as u64;
                    let worst = m.nbrs[x].iter().filter_map(|&e| cell[e].filter(|c| c.owner == Some(r))).map(|c| c.troops as u64).max().unwrap_or(0);
                    let tb = if def <= 10 { 0 } else if def <= 40 { 1 } else if def <= 150 { 2 } else { 3 };
                    let rb = if worst < 4 * def { 0 } else if worst < 10 * def { 1 } else if worst < 30 * def { 2 } else { 3 };
                    self.calib.push((view.tick, x, tb * 4 + rb));
                    self.calib_hits[tb * 4 + rb].0 += 1;
                }
                if self.debug_on && village(x).is_some() {
                    eprintln!("  t{} direct x={:?} troops {}", view.tick, m.hexes[x], cell[x].map_or(0, |c| c.troops));
                }
                consider(x, &mut best);
                if Some(x) == busy || floods >= 8 {
                    continue;
                }
                for &n in &m.nbrs[x] {
                    if blocked[n] || !cell[n].is_some_and(|c| c.owner == Some(me)) {
                        continue;
                    }
                    floods += 1;
                    if self.tarjan.sealed(&b, &blocked, x, n, 400) {
                        if self.debug_on {
                            eprintln!("  t{} pocket via x={:?} (troops {}) by p{} size {}", view.tick, m.hexes[x], cell[x].map_or(0, |c| c.troops), r, self.tarjan.flooded().len());
                        }
                        for &j in self.tarjan.flooded() {
                            consider(j as usize, &mut best);
                        }
                    }
                }
            }
        }
        self.board = Some(b);
        self.danger = danger;
        best.map(|(_, i)| m.hexes[i])
    }
}

impl Drop for Wildcard {
    fn drop(&mut self) {
        if let Some((n, total, worst)) = self.cpu {
            eprintln!(
                "cpu {} moves mean {:.3} ms worst {:.3} ms | {:?}",
                n,
                total as f64 / n.max(1) as f64 / 1e6,
                worst as f64 / 1e6,
                self.inner.stats
            );
        }
        if self.calib_on {
            let cells: Vec<String> = self.calib_hits.iter().enumerate().filter(|(_, h)| h.0 > 0).map(|(i, h)| format!("d{}r{}:{}/{}", i / 4, i % 4, h.1, h.0)).collect();
            eprintln!("calib {}", cells.join(" "));
        }
    }
}

impl Bot for Wildcard {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        let Some((n, total, worst)) = self.cpu else { return self.decide(view) };
        let start = thread_cpu_ns();
        let cmd = self.decide(view);
        let took = thread_cpu_ns().saturating_sub(start);
        self.cpu = Some((n + 1, total + took, worst.max(took)));
        cmd
    }
}

impl Wildcard {
    /// `style` with every knob named `<prefix><knob>` overriding `<knob>`.
    fn for_format(style: &Style, prefix: &str) -> Style {
        let mut s = style.clone();
        let over: Vec<(String, i64)> = style
            .params
            .knobs
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(prefix).map(|b| (b.to_string(), *v)))
            .collect();
        for (k, v) in over {
            s.params.knobs.insert(k, v);
        }
        s
    }

    fn decide(&mut self, view: &View) -> PlayerCommand {
        if let Some(style) = self.style.take() {
            let duel = view.public_players.len() == 2;
            let s = Self::for_format(&style, if duel { "duel_" } else { "ffa_" });
            self.inner = Core::new(s.clone());
            self.k = Knobs::read(&s);
            self.duel_when = if duel { 0 } else { s.params.knob("duel_when", 0).max(0) as usize };
            self.duel_style = (self.duel_when > 0).then(|| Self::for_format(&style, "duel_"));
        }
        if self.duel_when > 0 && view.public_players.iter().filter(|p| p.alive).count() <= self.duel_when {
            if let Some(s) = self.duel_style.take() {
                self.inner.retune(&s);
                self.k = Knobs::read(&s);
                self.duel_when = 0;
            }
        }
        if let Some(r) = &view.rules {
            self.rules = r.clone();
        }
        let mut cmd = self.inner.act(view);
        let m = self.map.take().unwrap_or_else(|| Map::new(view));
        if self.vclass.is_none() {
            let start = view.visible_hexes.iter().find(|c| c.owner == Some(view.you)).and_then(|c| m.slot(c.hex));
            self.vclass = start.map(|s| self.pick_class(&m, s));
            self.start = start;
            self.turtling = self.k.turtle & 1 != 0 && start.is_some_and(|s| self.exposed(&m, s));
        }
        if self.k.turtle > 0 && !self.turtling {
            self.update_turtle(view, &m);
        }
        let open = view.tick >= self.k.build_from
            && (self.k.build_until == 0 || view.tick < self.k.build_until)
            && self.last_build.is_none_or(|t| view.tick >= t + self.k.build_gap);
        let capture = match &cmd.action {
            Action::Move { to, .. } => {
                view.visible_hexes.iter().find(|c| c.hex == *to).is_some_and(|c| c.owner != Some(view.you))
            }
            Action::Gather { .. } | Action::Build { .. } | Action::Demolish { .. } | Action::NoOp => false,
        };
        if self.calib_on {
            let me = view.you;
            let mut keep = vec![];
            for &(t, i, bk) in &self.calib {
                let lost = view.visible_hexes.iter().find(|c| m.slot(c.hex) == Some(i)).is_none_or(|c| c.owner != Some(me));
                if lost {
                    self.calib_hits[bk].1 += 1;
                } else if view.tick < t + 10 {
                    keep.push((t, i, bk));
                }
            }
            self.calib = keep;
        }
        let ring_capture = self.inner.chosen.is_some();
        let doomed = if self.k.demolish { self.endangered_village(view, &m, &cmd.action) } else { None };
        let mut built = false;
        if let (true, Some(vclass), false) = (open, self.vclass, ring_capture) {
            if let Some((roi, a)) = self.build(view, &m, vclass) {
                if !capture || roi >= self.k.rush_roi {
                    cmd.action = a;
                    self.last_build = Some(view.tick);
                    built = true;
                }
            }
        }
        let turtle = self.turtling && (self.k.wall_from == 0 || view.tick < self.k.wall_from);
        let (from, cap, gap) = if turtle {
            (self.k.turtle_wall_from, self.k.turtle_cap, self.k.turtle_gap)
        } else {
            (self.k.wall_from, self.k.wall_cap, self.k.wall_gap)
        };
        let walls = from > 0 && view.tick >= from;
        if walls && !built && !capture && !ring_capture && self.last_wall.is_none_or(|t| view.tick >= t + gap) {
            if let Some(a) = self.wall(view, &m, cap) {
                cmd.action = a;
                self.last_wall = Some(view.tick);
            }
        }
        if walls {
            cmd.management.extend(self.links(view, &m));
        }
        if self.k.demolish
            && !ring_capture
            && self.last_demolish.is_none_or(|t| view.tick >= t + self.k.demolish_gap)
        {
            if let Some(at) = doomed {
                if self.debug_on {
                    let c = view.visible_hexes.iter().find(|c| c.hex == at).unwrap();
                    eprintln!("t{} p{} demolish {:?} income {} troops {}", view.tick, view.you, at, c.income, c.troops);
                }
                cmd.action = Action::Demolish { at };
                self.last_demolish = Some(view.tick);
                if let Some(i) = m.slot(at) {
                    self.banned.resize(m.passable.len(), 0);
                    self.banned[i] = view.tick + self.k.demolish_ban;
                }
            }
        }
        self.map = Some(m);
        cmd
    }
}
