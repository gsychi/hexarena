//! Pangolin: Ridgeline's border play on top of a Farm-Village lattice.
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
//! The tick goes to a build instead of Ridgeline's move when that move isn't a capture,
//! or when the build pays back faster than `rush_roi`.

use super::ridgeline::Ridgeline;
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
        }
    }
}

pub struct Pangolin {
    inner: Ridgeline,
    rules: RulesConfig,
    k: Knobs,
    map: Option<Map>,
    /// Lattice colour that holds Villages.
    vclass: Option<i32>,
    last_build: Option<u32>,
    last_wall: Option<u32>,
    start: Option<usize>,
    turtling: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Site {
    Empty,
    Mine(Building),
    Other,
}

impl Pangolin {
    pub fn new(style: Style) -> Pangolin {
        Pangolin {
            inner: Ridgeline::new(style.clone()),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            map: None,
            vclass: None,
            last_build: None,
            last_wall: None,
            start: None,
            turtling: false,
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
        let near_quarry = |i: usize| m.nbrs[i].iter().any(|&n| site[n] == Site::Mine(Building::Quarry));
        let cost = |b: Building, i: usize| self.rules.build_cost(b, near_quarry(i)).troops;
        let fut = self.k.future_pct * 10;
        let mut best: Option<(u64, usize, Building)> = None;
        for i in 0..slots {
            if site[i] != Site::Empty || depth[i] < self.k.depth || depth[i] == u32::MAX {
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

impl Bot for Pangolin {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
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
            Action::Gather { .. } | Action::Build { .. } | Action::NoOp => false,
        };
        let mut built = false;
        if let (true, Some(vclass)) = (open, self.vclass) {
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
        if walls && !built && !capture && self.last_wall.is_none_or(|t| view.tick >= t + gap) {
            if let Some(a) = self.wall(view, &m, cap) {
                cmd.action = a;
                self.last_wall = Some(view.tick);
            }
        }
        if walls {
            cmd.management.extend(self.links(view, &m));
        }
        self.map = Some(m);
        cmd
    }
}
