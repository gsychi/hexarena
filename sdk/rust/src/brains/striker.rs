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

use super::strike_core::StrikeCore;
use crate::{Action, Bot, Building, Hex, PlayerCommand, PlayerId, RulesConfig, Style, View};
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
        }
    }
}

pub struct Striker {
    inner: StrikeCore,
    rules: RulesConfig,
    k: Knobs,
    map: Option<Map>,
    /// Lattice colour that holds Villages.
    vclass: Option<i32>,
    last_build: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Site {
    Empty,
    Mine(Building),
    Other,
}

impl Striker {
    pub fn new(style: Style) -> Striker {
        Striker {
            inner: StrikeCore::new(style.clone()),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            map: None,
            vclass: None,
            last_build: None,
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
                    + self.rules.village.farm_bonus as u64 * count(Building::Farm))
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
                    if saved * 10 >= c * self.k.quarry_min_x10 {
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
}

impl Bot for Striker {
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
        }
        let open = view.tick >= self.k.build_from
            && (self.k.build_until == 0 || view.tick < self.k.build_until)
            && self.last_build.is_none_or(|t| view.tick >= t + self.k.build_gap);
        if let (true, Some(vclass)) = (open, self.vclass) {
            let capture = match &cmd.action {
                Action::Move { to, .. } => {
                    view.visible_hexes.iter().find(|c| c.hex == *to).is_some_and(|c| c.owner != Some(view.you))
                }
                Action::Gather { .. } | Action::Build { .. } | Action::NoOp => false,
            };
            if let Some((roi, a)) = self.build(view, &m, vclass) {
                if !capture || roi >= self.k.rush_roi {
                    cmd.action = a;
                    self.last_build = Some(view.tick);
                }
            }
        }
        self.map = Some(m);
        cmd
    }
}
