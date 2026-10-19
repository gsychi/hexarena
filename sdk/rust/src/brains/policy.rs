//! Policy: a learned action scorer.
//!
//! Every tick the bot lists the actions worth considering (captures at a few send sizes,
//! relays one hex toward the front, every affordable build, Gathers onto border hexes,
//! and doing nothing), describes each with a fixed-length feature vector, and plays the
//! one a small neural network scores highest. The network's weights are data, embedded
//! in the binary (`policy_model.json`) or loaded from `HEXARENA_POLICY_MODEL`; they were
//! fitted by imitating the top finishers of Hexarena's own v0.6 games and then tuned by
//! self-play. Only Wall links are hand-written: every adjacent pair of our Walls with a
//! free link gets linked, since a link is free and never hurts.

use crate::{Action, Bot, Building, Hex, ManagementCommand, PlayerCommand, RulesConfig, Style, View, MILLI};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Global features, shared by every candidate of a tick.
pub const NG: usize = 16;
/// Candidate kinds (one-hot).
pub const NK: usize = 9;
/// Per-candidate features.
pub const NC: usize = 50;
/// Total feature length.
pub const F: usize = NG + NK + NC;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    CapNeutral,
    CapEnemy,
    Relay,
    Farm,
    Village,
    Quarry,
    Wall,
    Gather,
    NoOp,
}

impl Kind {
    pub const ALL: [Kind; NK] = [
        Kind::CapNeutral,
        Kind::CapEnemy,
        Kind::Relay,
        Kind::Farm,
        Kind::Village,
        Kind::Quarry,
        Kind::Wall,
        Kind::Gather,
        Kind::NoOp,
    ];

    pub fn index(self) -> usize {
        match self {
            Kind::CapNeutral => 0,
            Kind::CapEnemy => 1,
            Kind::Relay => 2,
            Kind::Farm => 3,
            Kind::Village => 4,
            Kind::Quarry => 5,
            Kind::Wall => 6,
            Kind::Gather => 7,
            Kind::NoOp => 8,
        }
    }

    fn of_building(b: Building) -> Kind {
        match b {
            Building::Farm => Kind::Farm,
            Building::Village => Kind::Village,
            Building::Quarry => Kind::Quarry,
            Building::Wall => Kind::Wall,
        }
    }
}

/// One candidate action: `a`/`b` are board slots (source and target of a move, the site
/// of a build, the target of a Gather).
#[derive(Clone, Copy, Debug)]
pub struct Cand {
    pub kind: Kind,
    pub a: usize,
    pub b: usize,
    pub troops: u32,
}

/// What the bot remembers of its own past actions.
#[derive(Clone, Copy, Debug, Default)]
pub struct History {
    pub last_build: Option<u32>,
    pub last_gather: Option<u32>,
    pub last_capture: Option<u32>,
}

impl History {
    pub fn record(&mut self, tick: u32, action: &Action, captured: bool) {
        match action {
            Action::Build { .. } => self.last_build = Some(tick),
            Action::Gather { .. } => self.last_gather = Some(tick),
            Action::Move { .. } if captured => self.last_capture = Some(tick),
            _ => {}
        }
    }
}

/// Map-static data from the first view.
pub struct Geometry {
    pub radius: i32,
    width: i32,
    pub passable: Vec<bool>,
    pub height: Vec<i8>,
    pub nbrs: Vec<Vec<usize>>,
    pub hexes: Vec<Hex>,
    /// Neighbour slots on the board, passable or not, and how many of the six are off it.
    ring: Vec<Vec<usize>>,
    offboard: Vec<u8>,
    colour: Vec<u8>,
    passable_total: usize,
}

impl Geometry {
    pub fn new(view: &View) -> Geometry {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let mut g = Geometry {
            radius,
            width,
            passable: vec![false; slots],
            height: vec![0; slots],
            nbrs: vec![],
            hexes: (0..slots).map(|i| Hex::new(i as i32 % width - radius, i as i32 / width - radius)).collect(),
            ring: vec![],
            offboard: vec![0; slots],
            colour: vec![0; slots],
            passable_total: 0,
        };
        for i in 0..slots {
            if let Some(t) = view.public_terrain[i] {
                g.passable[i] = t.passable();
                g.height[i] = t.height;
            }
            let h = g.hexes[i];
            g.colour[i] = (h.q - h.r).rem_euclid(3) as u8;
        }
        g.passable_total = g.passable.iter().filter(|&&p| p).count();
        let mut ring = vec![];
        let mut nbrs = vec![];
        for i in 0..slots {
            let on: Vec<usize> = g.hexes[i].neighbors().iter().filter_map(|n| g.slot(*n)).collect();
            g.offboard[i] = (6 - on.len()) as u8;
            nbrs.push(on.iter().copied().filter(|&j| g.passable[j]).collect());
            ring.push(on);
        }
        g.nbrs = nbrs;
        g.ring = ring;
        g
    }

    pub fn slot(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * self.width + h.q + r) as usize)
    }
}

const NONE: u16 = u16::MAX;
/// `owner` value for hexes nobody holds.
const NEUTRAL: i32 = -1;

/// The small network: `score = lin . x + w2 . relu(W1 x + b1) + b2` on standardized
/// features `x = (raw - mean) * inv_std`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub f: usize,
    pub h: usize,
    pub mean: Vec<f32>,
    pub inv_std: Vec<f32>,
    pub lin: Vec<f32>,
    /// Row-major `h x f`.
    pub w1: Vec<f32>,
    pub b1: Vec<f32>,
    pub w2: Vec<f32>,
    pub b2: f32,
}

impl Model {
    pub fn zeros(h: usize) -> Model {
        Model {
            f: F,
            h,
            mean: vec![0.0; F],
            inv_std: vec![1.0; F],
            lin: vec![0.0; F],
            w1: vec![0.0; h * F],
            b1: vec![0.0; h],
            w2: vec![0.0; h],
            b2: 0.0,
        }
    }

    pub fn embedded() -> Model {
        if let Ok(path) = std::env::var("HEXARENA_POLICY_MODEL") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(m) = serde_json::from_str::<Model>(&text) {
                    return m;
                }
            }
            eprintln!("policy: could not read model {path}; using the embedded one");
        }
        serde_json::from_str(include_str!("policy_model.json")).expect("embedded policy model")
    }

    /// Parameters as one flat vector (lin, w1, b1, w2, b2), for evolution strategies.
    pub fn params(&self) -> Vec<f32> {
        let mut v = self.lin.clone();
        v.extend(&self.w1);
        v.extend(&self.b1);
        v.extend(&self.w2);
        v.push(self.b2);
        v
    }

    pub fn set_params(&mut self, p: &[f32]) {
        let (f, h) = (self.f, self.h);
        self.lin.copy_from_slice(&p[..f]);
        self.w1.copy_from_slice(&p[f..f + h * f]);
        self.b1.copy_from_slice(&p[f + h * f..f + h * f + h]);
        self.w2.copy_from_slice(&p[f + h * f + h..f + h * f + 2 * h]);
        self.b2 = p[f + h * f + 2 * h];
    }

    /// Scores every row of `feats` (`n x F`).
    pub fn score_all(&self, feats: &[f32], out: &mut Vec<f32>) {
        let f = self.f;
        out.clear();
        let mut x = vec![0f32; f];
        for row in feats.chunks_exact(f) {
            for k in 0..f {
                x[k] = (row[k] - self.mean[k]) * self.inv_std[k];
            }
            let mut s = self.b2;
            for k in 0..f {
                s += self.lin[k] * x[k];
            }
            for j in 0..self.h {
                let w = &self.w1[j * f..(j + 1) * f];
                let mut a = self.b1[j];
                for k in 0..f {
                    a += w[k] * x[k];
                }
                if a > 0.0 {
                    s += self.w2[j] * a;
                }
            }
            out.push(s);
        }
    }
}

/// A [`Model`] with standardization folded into its weights, laid out for sparse rows:
/// a raw feature of 0 costs nothing, and the global features (the same for every
/// candidate of a tick) are applied once per tick.
pub struct Folded {
    h: usize,
    /// Column-major first layer: `wt[k * h + j]`.
    wt: Vec<f32>,
    b1: Vec<f32>,
    lin: Vec<f32>,
    w2: Vec<f32>,
    b2: f32,
}

impl Folded {
    pub fn new(m: &Model) -> Folded {
        let (f, h) = (m.f, m.h);
        let mut wt = vec![0f32; f * h];
        let mut b1 = m.b1.clone();
        let mut lin = vec![0f32; f];
        let mut b2 = m.b2;
        for k in 0..f {
            lin[k] = m.lin[k] * m.inv_std[k];
            b2 -= lin[k] * m.mean[k];
            for j in 0..h {
                let w = m.w1[j * f + k] * m.inv_std[k];
                wt[k * h + j] = w;
                b1[j] -= w * m.mean[k];
            }
        }
        Folded { h, wt, b1, lin, w2: m.w2.clone(), b2 }
    }

    /// Scores every row of `feats` (`n x F`); all rows share their first `NG` features.
    pub fn score_all(&self, feats: &[f32], out: &mut Vec<f32>) {
        out.clear();
        let h = self.h;
        let Some(first) = feats.get(..F) else { return };
        let mut pre0 = self.b1.clone();
        let mut s0 = self.b2;
        for k in 0..NG {
            let v = first[k];
            if v != 0.0 {
                s0 += self.lin[k] * v;
                for (a, w) in pre0.iter_mut().zip(&self.wt[k * h..(k + 1) * h]) {
                    *a += w * v;
                }
            }
        }
        let mut acc = vec![0f32; h];
        for row in feats.chunks_exact(F) {
            acc.copy_from_slice(&pre0);
            let mut s = s0;
            for k in NG..F {
                let v = row[k];
                if v != 0.0 {
                    s += self.lin[k] * v;
                    for (a, w) in acc.iter_mut().zip(&self.wt[k * h..(k + 1) * h]) {
                        *a += w * v;
                    }
                }
            }
            for (a, w) in acc.iter().zip(&self.w2) {
                s += w * a.max(0.0);
            }
            out.push(s);
        }
    }
}

/// Builds the candidate list and its features for one view. Keeps its buffers between
/// ticks so a turn allocates almost nothing.
pub struct Featurizer {
    pub rules: RulesConfig,
    pub geo: Option<Geometry>,
    pub cands: Vec<Cand>,
    pub feats: Vec<f32>,
    owner: Vec<i32>,
    troops: Vec<u32>,
    building: Vec<Option<Building>>,
    wall_milli: Vec<u32>,
    wall_links: Vec<u8>,
    income: Vec<u32>,
    /// Steps from land we don't own, through our land (1 = border).
    depth: Vec<u16>,
    /// Steps from our nearest enemy-facing hex, through our land.
    tdist: Vec<u16>,
    /// Garrison wanted against the adjacent enemy stacks (0 = no enemy next to it).
    want: Vec<u64>,
    /// Steps to the nearest of our border hexes holding less than it wants, and that
    /// hex's shortfall (0..1).
    dshort: Vec<u16>,
    short_of: Vec<f32>,
    /// That hex itself.
    short_src: Vec<usize>,
    /// Steps to the border hex facing the cheapest hex we could take.
    dpad: Vec<u16>,
    pad: usize,
    enclose_cache: Vec<i32>,
    win: u64,
}

impl Default for Featurizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Approximate standard normal CDF.
fn phi(z: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * z.abs());
    let d = 0.398_942_280_4 * (-z * z / 2.0).exp();
    let p = d * t * (0.319_381_53 + t * (-0.356_563_782 + t * (1.781_477_937 + t * (-1.821_255_978 + t * 1.330_274_429))));
    if z > 0.0 {
        1.0 - p
    } else {
        p
    }
}

fn ln1p(x: f64) -> f32 {
    (x.max(0.0)).ln_1p() as f32
}

impl Featurizer {
    pub fn new() -> Featurizer {
        Featurizer {
            rules: RulesConfig::default(),
            geo: None,
            cands: vec![],
            feats: vec![],
            owner: vec![],
            troops: vec![],
            building: vec![],
            wall_milli: vec![],
            wall_links: vec![],
            income: vec![],
            depth: vec![],
            tdist: vec![],
            want: vec![],
            dshort: vec![],
            short_of: vec![],
            short_src: vec![],
            dpad: vec![],
            pad: usize::MAX,
            enclose_cache: vec![],
            win: 470,
        }
    }

    pub fn hex(&self, slot: usize) -> Hex {
        self.geo.as_ref().unwrap().hexes[slot]
    }

    pub fn action(&self, c: &Cand) -> Action {
        let g = self.geo.as_ref().unwrap();
        match c.kind {
            Kind::CapNeutral | Kind::CapEnemy | Kind::Relay => {
                Action::Move { from: g.hexes[c.a], to: g.hexes[c.b], troops: c.troops }
            }
            Kind::Farm => Action::Build { at: g.hexes[c.a], building: Building::Farm },
            Kind::Village => Action::Build { at: g.hexes[c.a], building: Building::Village },
            Kind::Quarry => Action::Build { at: g.hexes[c.a], building: Building::Quarry },
            Kind::Wall => Action::Build { at: g.hexes[c.a], building: Building::Wall },
            Kind::Gather => Action::Gather { target: g.hexes[c.a] },
            Kind::NoOp => Action::NoOp,
        }
    }

    /// The candidate that best stands for `action`: same kind and hexes, nearest troop
    /// count on a log scale. `None` when the action isn't among the candidates.
    pub fn find(&self, action: &Action) -> Option<usize> {
        let g = self.geo.as_ref()?;
        let mut best: Option<(f64, usize)> = None;
        for (i, c) in self.cands.iter().enumerate() {
            let d = match (action, c.kind) {
                (Action::NoOp, Kind::NoOp) => 0.0,
                (Action::Gather { target }, Kind::Gather) if g.slot(*target) == Some(c.a) => 0.0,
                (Action::Build { at, building }, k) if Kind::of_building(*building) == k && g.slot(*at) == Some(c.a) => 0.0,
                (Action::Move { from, to, troops }, Kind::CapNeutral | Kind::CapEnemy | Kind::Relay)
                    if g.slot(*from) == Some(c.a) && g.slot(*to) == Some(c.b) =>
                {
                    ((*troops as f64).max(1.0).ln() - (c.troops as f64).max(1.0).ln()).abs()
                }
                _ => continue,
            };
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    fn loss_stats(&self, def: u32, defense: u64) -> (f64, f64) {
        let d = defense.max(1);
        let per_win = (1_000_000 + d / 2) / d;
        let wins = (def as u64 * 1000).div_ceil(per_win.max(1)) as f64;
        let p = self.win as f64 / 1000.0;
        let q = 1.0 - p;
        (wins * q / p, (wins * q).sqrt() / p)
    }

    fn kills(&self, attackers: u64, defense: u64) -> u64 {
        let p = self.win;
        attackers * p * 1000 / ((1000 - p) * defense.max(1))
    }

    fn defense(&self, from: usize, to: usize) -> u64 {
        let g = self.geo.as_ref().unwrap();
        let h = self.rules.height_multiplier_milli(g.height[from], g.height[to]) as u64;
        (h * self.wall_milli[to] as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64)
    }

    /// Garrison wanted at `h` against the enemy stacks next to it.
    fn want_at(&self, h: usize, me: i32) -> u64 {
        let g = self.geo.as_ref().unwrap();
        let mut worst = 0u64;
        let mut any = false;
        for &n in &g.nbrs[h] {
            let o = self.owner[n];
            if o == NEUTRAL || o == me {
                continue;
            }
            any = true;
            let stack = self.troops[n].saturating_sub(1) as u64;
            let h_mult = self.rules.height_multiplier_milli(g.height[n], g.height[h]) as u64;
            let wall = if self.building[h] == Some(Building::Wall) { self.wall_milli[h] as u64 } else { MILLI as u64 };
            let d = (h_mult * wall / 1000).min(self.rules.maximum_total_defense_milli as u64);
            worst = worst.max(self.kills(stack, d));
        }
        if any {
            10 + worst * 2
        } else {
            0
        }
    }

    fn safety(troops: u64, want: u64) -> f32 {
        if want == 0 {
            return 1.0;
        }
        let r = (troops as f64 / want as f64).min(1.0);
        (1.0 - (1.0 - r) * (1.0 - r)) as f32
    }

    /// Hexes of rival pockets that taking `to` would make surrender.
    fn enclosure(&self, to: usize, me: i32) -> i32 {
        const LIMIT: usize = 48;
        let g = self.geo.as_ref().unwrap();
        let mut gain = 0;
        let mut done: Vec<usize> = vec![];
        for &n in &g.ring[to] {
            let o = self.owner[n];
            if o == NEUTRAL || o == me || done.contains(&n) || !g.passable[n] {
                continue;
            }
            let mut comp = vec![n];
            let mut k = 0;
            let mut enclosed = g.offboard[n] == 0;
            while k < comp.len() && enclosed {
                let c = comp[k];
                k += 1;
                if g.offboard[c] > 0 {
                    enclosed = false;
                    break;
                }
                for &j in &g.ring[c] {
                    if j == to {
                        continue;
                    }
                    if !g.passable[j] {
                        if self.rules.impassable_breaks_encirclement {
                            enclosed = false;
                            break;
                        }
                        continue;
                    }
                    let oj = self.owner[j];
                    if oj == o {
                        if !comp.contains(&j) {
                            comp.push(j);
                        }
                    } else if oj != me {
                        enclosed = false;
                        break;
                    }
                }
                if comp.len() > LIMIT {
                    enclosed = false;
                }
            }
            done.extend(&comp);
            if enclosed {
                gain += comp.len() as i32;
            }
        }
        gain
    }

    fn bfs(&self, seeds: impl Iterator<Item = usize>, through: impl Fn(usize) -> bool, out: &mut Vec<u16>) {
        let g = self.geo.as_ref().unwrap();
        out.clear();
        out.resize(g.passable.len(), NONE);
        let mut q = VecDeque::new();
        for s in seeds {
            out[s] = 0;
            q.push_back(s);
        }
        while let Some(c) = q.pop_front() {
            for &n in &g.nbrs[c] {
                if out[n] == NONE && through(n) {
                    out[n] = out[c] + 1;
                    q.push_back(n);
                }
            }
        }
    }

    /// Lists this tick's candidates into `cands` and their features into `feats`.
    pub fn build(&mut self, view: &View, hist: &History) {
        if let Some(r) = &view.rules {
            self.rules = r.clone();
        }
        if self.geo.is_none() {
            self.geo = Some(Geometry::new(view));
        }
        self.win = self.rules.attacker_win_milli(view.tick) as u64;
        let slots = self.geo.as_ref().unwrap().passable.len();
        let me = view.you as i32;
        self.owner.clear();
        self.owner.resize(slots, NEUTRAL);
        self.troops.clear();
        self.troops.resize(slots, 0);
        self.building.clear();
        self.building.resize(slots, None);
        self.wall_milli.clear();
        self.wall_milli.resize(slots, MILLI);
        self.wall_links.clear();
        self.wall_links.resize(slots, 0);
        self.income.clear();
        self.income.resize(slots, 0);
        let mut mine: Vec<usize> = vec![];
        let mut own_troops = 0u64;
        let mut own_income = 0u64;
        let mut own_buildings = 0usize;
        // Colour classes of our Villages and Farms, for lattice conformity.
        let mut vcol = [0u32; 3];
        let mut fcol = [0u32; 3];
        let mut alive = 0usize;
        for p in &view.public_players {
            alive += p.alive as usize;
        }
        {
            let g = self.geo.as_ref().unwrap();
            for c in &view.visible_hexes {
                let Some(i) = g.slot(c.hex) else { continue };
                self.owner[i] = c.owner.map_or(NEUTRAL, |o| o as i32);
                self.troops[i] = c.troops;
                self.building[i] = c.building.as_ref().map(|b| b.kind);
                self.wall_milli[i] = c.wall_milli;
                self.wall_links[i] = c.building.as_ref().map_or(0, |b| if b.kind == Building::Wall { b.links.len() as u8 } else { 0 });
                self.income[i] = c.income;
                if c.owner == Some(view.you) {
                    mine.push(i);
                    own_troops += c.troops as u64;
                    own_income += c.income as u64;
                    if let Some(b) = &c.building {
                        own_buildings += 1;
                        match b.kind {
                            Building::Village => vcol[g.colour[i] as usize] += 1,
                            Building::Farm => fcol[g.colour[i] as usize] += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        let mut depth = std::mem::take(&mut self.depth);
        let mut tdist = std::mem::take(&mut self.tdist);
        {
            let g = self.geo.as_ref().unwrap();
            let owner = &self.owner;
            self.bfs((0..slots).filter(|&i| g.passable[i] && owner[i] != me), |n| owner[n] == me, &mut depth);
            let facing = mine.iter().copied().filter(|&s| g.nbrs[s].iter().any(|&n| owner[n] != me && owner[n] != NEUTRAL));
            self.bfs(facing, |n| owner[n] == me, &mut tdist);
        }
        self.depth = depth;
        self.tdist = tdist;
        self.want.clear();
        self.want.resize(slots, 0);
        let mut contact = false;
        let mut frontier = 0usize;
        let mut facing = 0usize;
        for &s in &mine {
            let w = self.want_at(s, me);
            self.want[s] = w;
            if self.depth[s] == 1 {
                frontier += 1;
            }
            if w > 0 {
                contact = true;
                facing += 1;
            }
        }
        self.enclose_cache.clear();
        self.enclose_cache.resize(slots, -1);
        {
            let g = self.geo.as_ref().unwrap();
            let mut dshort = std::mem::take(&mut self.dshort);
            let mut short_of = std::mem::take(&mut self.short_of);
            let mut short_src = std::mem::take(&mut self.short_src);
            short_src.clear();
            short_src.resize(slots, usize::MAX);
            dshort.clear();
            dshort.resize(slots, NONE);
            short_of.clear();
            short_of.resize(slots, 0.0);
            let mut q = VecDeque::new();
            let mut cheapest = (u32::MAX, usize::MAX);
            for &s in &mine {
                let w = self.want[s];
                if w > 0 && (self.troops[s] as u64) < w {
                    dshort[s] = 0;
                    short_of[s] = 1.0 - self.troops[s] as f32 / w as f32;
                    short_src[s] = s;
                    q.push_back(s);
                }
                if self.depth[s] == 1 {
                    for &n in &g.nbrs[s] {
                        if self.owner[n] != me && (self.troops[n], s) < cheapest {
                            cheapest = (self.troops[n], s);
                        }
                    }
                }
            }
            while let Some(c) = q.pop_front() {
                for &n in &g.nbrs[c] {
                    if dshort[n] == NONE && self.owner[n] == me {
                        dshort[n] = dshort[c] + 1;
                        short_of[n] = short_of[c];
                        short_src[n] = short_src[c];
                        q.push_back(n);
                    }
                }
            }
            self.dshort = dshort;
            self.short_of = short_of;
            self.short_src = short_src;
            self.pad = cheapest.1;
        }
        let mut dpad = std::mem::take(&mut self.dpad);
        {
            let owner = &self.owner;
            let pad = self.pad;
            self.bfs((pad != usize::MAX).then_some(pad).into_iter(), |n| owner[n] == me, &mut dpad);
        }
        self.dpad = dpad;

        let tick = view.tick;
        let left = self.rules.max_ticks.saturating_sub(tick) as f32;
        let land = mine.len() as f64;
        let gl: [f32; NG] = {
            let g = self.geo.as_ref().unwrap();
            let since = |t: Option<u32>| t.map_or(1.0, |t| ((tick - t.min(tick)) as f32 / 30.0).min(1.0));
            [
                tick as f32 / 10000.0,
                (self.win as f32 - 470.0) / 380.0,
                ((1500.0 - left) / 1500.0).clamp(0.0, 1.0),
                ((300.0 - left) / 300.0).clamp(0.0, 1.0),
                ln1p(land) / 6.0,
                ln1p(own_troops as f64) / 15.0,
                ln1p(own_troops as f64 / land.max(1.0)) / 10.0,
                own_buildings as f32 / land.max(1.0) as f32,
                contact as u8 as f32,
                alive as f32 / view.public_players.len().max(1) as f32,
                (land * view.public_players.len() as f64 / g.passable_total.max(1) as f64) as f32,
                facing as f32 / frontier.max(1) as f32,
                since(hist.last_build),
                since(hist.last_gather),
                since(hist.last_capture),
                ln1p(own_income as f64) / 8.0,
            ]
        };

        self.cands.clear();
        self.feats.clear();
        let mut row = [0f32; F];
        let push = |s: &mut Featurizer, c: Cand, row: &mut [f32; F]| {
            row[..NG].copy_from_slice(&gl);
            for k in 0..NK {
                row[NG + k] = 0.0;
            }
            row[NG + c.kind.index()] = 1.0;
            s.cands.push(c);
            s.feats.extend_from_slice(row);
        };
        const C: usize = NG + NK;
        let vtot = (vcol[0] + vcol[1] + vcol[2]).max(1) as f32;
        let ftot = (fcol[0] + fcol[1] + fcol[2]).max(1) as f32;

        // Moves: captures and relays.
        for &s in &mine {
            let ts = self.troops[s];
            if ts < 2 {
                continue;
            }
            let avail = ts as u64 - 1;
            let g = self.geo.as_ref().unwrap();
            let nb: Vec<usize> = g.nbrs[s].clone();
            let src_height = g.height[s];
            let ws = self.want[s];
            let before = Self::safety(ts as u64, ws);
            let hexf = |f: &Featurizer, row: &mut [f32; F], i: usize| {
                let t = f.troops[i] as f64;
                row[C] = ln1p(t) / 12.0;
                row[C + 1] = (f.want[i] > 0) as u8 as f32;
                row[C + 2] = (f.depth[i] == 1) as u8 as f32;
                row[C + 3] = (f.depth[i].min(8) as f32) / 8.0;
                row[C + 4] = Self::safety(f.troops[i] as u64, f.want[i]);
                row[C + 5] = if f.want[i] > 0 { ((t + 1.0) / f.want[i] as f64).ln().clamp(-4.0, 4.0) as f32 / 4.0 } else { 1.0 };
                row[C + 6] = (f.tdist[i].min(10) as f32) / 10.0;
                row[C + 44] = (f.dshort[i].min(10) as f32) / 10.0;
                row[C + 43] = f.short_of[i];
                row[C + 46] = (f.dpad[i].min(10) as f32) / 10.0;
                row[C + 47] = (i == f.pad) as u8 as f32;
            };
            for &t in &nb {
                let to = self.owner[t];
                for k in C..F {
                    row[k] = 0.0;
                }
                hexf(self, &mut row, s);
                if to == me {
                    // Relay toward the front or toward the nearest enemy-facing hex.
                    let closer = self.tdist[t] < self.tdist[s]
                        || self.depth[t] < self.depth[s]
                        || self.dshort[t] < self.dshort[s]
                        || self.dpad[t] < self.dpad[s];
                    if !closer {
                        continue;
                    }
                    let spare = (ts as u64).saturating_sub(ws.max(1));
                    row[C + 20] = self.depth[t] as f32 - self.depth[s] as f32;
                    let dt = |x: u16| if x == NONE { 30.0 } else { x as f32 };
                    row[C + 21] = (dt(self.tdist[t]) - dt(self.tdist[s])).clamp(-2.0, 2.0);
                    row[C + 22] = (self.want[t] > 0) as u8 as f32;
                    row[C + 23] = if self.want[t] > 0 {
                        (1.0 - self.troops[t] as f64 / self.want[t] as f64).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    };
                    row[C + 24] = (self.depth[t] == 1) as u8 as f32;
                    row[C + 42] = (dt(self.dshort[t]) - dt(self.dshort[s])).clamp(-2.0, 2.0);
                    row[C + 45] = (dt(self.dpad[t]) - dt(self.dpad[s])).clamp(-2.0, 2.0);
                    row[C + 25] = ln1p(self.troops[t] as f64) / 12.0;
                    let mut sends = [spare, avail];
                    if sends[0] < 1 {
                        sends[0] = avail;
                    }
                    let n = if sends[0] == sends[1] { 1 } else { 2 };
                    for &send in &sends[..n] {
                        row[C + 7] = send as f32 / ts as f32;
                        row[C + 8] = ln1p(send as f64) / 12.0;
                        row[C + 26] = Self::safety(ts as u64 - send, ws);
                        let o = self.short_src[s];
                        if o != usize::MAX && self.dshort[t] < self.dshort[s] {
                            let (have, w) = (self.troops[o] as u64, self.want[o]);
                            row[C + 48] = (send as f32 / w.saturating_sub(have).max(1) as f32).min(1.0);
                            row[C + 49] = (Self::safety(have + send, w) - Self::safety(have, w)) / self.dshort[s].max(1) as f32;
                        } else {
                            row[C + 48] = 0.0;
                            row[C + 49] = 0.0;
                        }
                        push(self, Cand { kind: Kind::Relay, a: s, b: t, troops: send as u32 }, &mut row);
                    }
                    continue;
                }
                let defense = self.defense(s, t);
                let (mean, sd) = self.loss_stats(self.troops[t], defense);
                let min_send = mean.ceil() as u64 + 1;
                if min_send > avail {
                    continue;
                }
                let need = (mean + 3.0 * sd).ceil() as u64 + 1;
                let g = self.geo.as_ref().unwrap();
                let hugged = g.nbrs[t].iter().filter(|&&n| self.owner[n] == me).count();
                let exposed = g.nbrs[t].len() - hugged;
                let mut enemy_n = 0usize;
                let mut threat_t = 0u64;
                for &n in &g.nbrs[t] {
                    let o = self.owner[n];
                    if o != me && o != NEUTRAL {
                        enemy_n += 1;
                        let hm = self.rules.height_multiplier_milli(g.height[n], g.height[t]) as u64;
                        threat_t = threat_t.max(self.kills(self.troops[n].saturating_sub(1) as u64, hm));
                    }
                }
                let want_t = if enemy_n > 0 { 10 + 2 * threat_t } else { 0 };
                let enemy = to != NEUTRAL;
                if enemy && self.enclose_cache[t] < 0 {
                    self.enclose_cache[t] = self.enclosure(t, me);
                }
                let enc = self.enclose_cache[t].max(0);
                let g = self.geo.as_ref().unwrap();
                row[C + 9] = ln1p(self.troops[t] as f64) / 12.0;
                row[C + 10] = defense as f32 / 5000.0;
                row[C + 12] = hugged as f32 / 6.0;
                row[C + 13] = exposed as f32 / 6.0;
                row[C + 14] = g.height[t] as f32 / 2.0;
                row[C + 15] = (g.height[t] - src_height) as f32 / 4.0;
                row[C + 16] = ln1p(enc as f64) / 4.0;
                row[C + 17] = enemy_n as f32 / 6.0;
                row[C + 18] = matches!(self.building[t], Some(Building::Village)) as u8 as f32;
                row[C + 19] = matches!(self.building[t], Some(Building::Farm | Building::Quarry | Building::Wall)) as u8 as f32;
                row[C + 27] = ln1p(mean) / 12.0;
                row[C + 28] = (g.offboard[t] as usize + g.ring[t].len() - g.nbrs[t].len()) as f32 / 6.0;
                let hold = want_t + mean.ceil() as u64;
                let mut sends: Vec<u64> = if need <= avail {
                    vec![need, hold.clamp(need, avail), (need + avail) / 2, avail]
                } else {
                    vec![avail]
                };
                sends.sort_unstable();
                sends.dedup();
                for send in sends {
                    let z = (send as f64 - 0.5 - mean) / sd.max(0.5);
                    row[C + 7] = send as f32 / ts as f32;
                    row[C + 8] = ln1p(send as f64) / 12.0;
                    row[C + 11] = phi(z) as f32;
                    row[C + 29] = (z.clamp(-3.0, 9.0) / 9.0) as f32;
                    let left_t = (send as f64 - mean).max(1.0) as u64;
                    row[C + 30] = Self::safety(left_t, want_t);
                    row[C + 26] = Self::safety(ts as u64 - send, ws);
                    row[C + 31] = before - row[C + 26];
                    row[C + 32] = if want_t > 0 { ((left_t as f64 + 1.0) / want_t as f64).ln().clamp(-4.0, 4.0) as f32 / 4.0 } else { 1.0 };
                    let kind = if enemy { Kind::CapEnemy } else { Kind::CapNeutral };
                    push(self, Cand { kind, a: s, b: t, troops: send as u32 }, &mut row);
                }
            }
        }

        // Builds.
        for &s in &mine {
            if self.building[s].is_some() {
                continue;
            }
            let g = self.geo.as_ref().unwrap();
            let mut adj = [0usize; 4];
            let mut empty = 0usize;
            let mut empty_farmable = 0usize;
            let mut walls_free = 0usize;
            let mut enemy_adj = 0usize;
            let mut threat = 0u64;
            let mut quarry_near = false;
            for &n in &g.nbrs[s] {
                if self.owner[n] == me {
                    match self.building[n] {
                        Some(Building::Farm) => adj[0] += 1,
                        Some(Building::Village) => adj[1] += 1,
                        Some(Building::Quarry) => {
                            adj[2] += 1;
                            quarry_near = true;
                        }
                        Some(Building::Wall) => {
                            adj[3] += 1;
                            if (self.wall_links[n] as usize) < self.rules.wall.max_links as usize {
                                walls_free += 1;
                            }
                        }
                        None => {
                            empty += 1;
                            if self.rules.terrain_allowed(Building::Farm, g.height[n]) {
                                empty_farmable += 1;
                            }
                        }
                    }
                } else if self.owner[n] != NEUTRAL {
                    enemy_adj += 1;
                    threat = threat.max(self.troops[n] as u64);
                }
            }
            let h = g.height[s];
            let col = g.colour[s] as usize;
            let ts = self.troops[s];
            let quarry_close = self.rules.terrain_allowed(Building::Quarry, h) && {
                let min = self.rules.quarry.min_friendly_distance;
                mine.iter().any(|&q| self.building[q] == Some(Building::Quarry) && g.hexes[q].distance(g.hexes[s]) < min)
            };
            for b in Building::ALL {
                if !self.rules.terrain_allowed(b, h) || (b == Building::Quarry && quarry_close) {
                    continue;
                }
                let cost = self.rules.build_cost(b, quarry_near).troops;
                if ts < cost + 1 {
                    continue;
                }
                let r = &self.rules;
                let gain = match b {
                    Building::Village => r.village.bonus as usize + r.village.farm_bonus as usize * adj[0] + r.quarry.village_bonus as usize * adj[2],
                    Building::Farm => r.village.farm_bonus as usize * adj[1],
                    Building::Quarry => r.quarry.village_bonus as usize * adj[1],
                    Building::Wall => 0,
                };
                for k in C..F {
                    row[k] = 0.0;
                }
                let t = ts as f64;
                row[C] = ln1p(t) / 12.0;
                row[C + 1] = (self.want[s] > 0) as u8 as f32;
                row[C + 2] = (self.depth[s] == 1) as u8 as f32;
                row[C + 3] = (self.depth[s].min(8) as f32) / 8.0;
                row[C + 4] = Self::safety(ts as u64, self.want[s]);
                row[C + 5] = if self.want[s] > 0 { ((t + 1.0) / self.want[s] as f64).ln().clamp(-4.0, 4.0) as f32 / 4.0 } else { 1.0 };
                row[C + 6] = (self.tdist[s].min(10) as f32) / 10.0;
                row[C + 7] = cost as f32 / ts as f32;
                row[C + 8] = ln1p(cost as f64) / 12.0;
                row[C + 14] = h as f32 / 2.0;
                row[C + 26] = Self::safety((ts - cost) as u64, self.want[s]);
                row[C + 33] = adj[0] as f32 / 6.0;
                row[C + 34] = adj[1] as f32 / 6.0;
                row[C + 35] = (adj[2] > 0) as u8 as f32;
                row[C + 36] = empty_farmable as f32 / 6.0 + empty as f32 / 36.0;
                row[C + 37] = gain as f32 * 30.0 / cost.max(1) as f32;
                row[C + 38] = vcol[col] as f32 / vtot;
                row[C + 39] = fcol[col] as f32 / ftot;
                row[C + 40] = ln1p(threat as f64) / 12.0;
                row[C + 41] = walls_free as f32 / 2.0;
                row[C + 17] = enemy_adj as f32 / 6.0;
                push(self, Cand { kind: Kind::of_building(b), a: s, b: s, troops: 0 }, &mut row);
            }
        }

        // Gathers onto border hexes.
        if mine.len() > 3 {
            for &s in &mine {
                if self.depth[s] != 1 {
                    continue;
                }
                let g = self.geo.as_ref().unwrap();
                let mut cheapest = u32::MAX;
                let mut neutral = 0usize;
                for &n in &g.nbrs[s] {
                    if self.owner[n] != me {
                        cheapest = cheapest.min(self.troops[n]);
                        neutral += (self.owner[n] == NEUTRAL) as usize;
                    }
                }
                for k in C..F {
                    row[k] = 0.0;
                }
                let t = self.troops[s] as f64;
                row[C] = ln1p(t) / 12.0;
                row[C + 1] = (self.want[s] > 0) as u8 as f32;
                row[C + 2] = 1.0;
                row[C + 3] = 1.0 / 8.0;
                row[C + 4] = Self::safety(self.troops[s] as u64, self.want[s]);
                row[C + 9] = ln1p(cheapest as f64) / 12.0;
                row[C + 13] = neutral as f32 / 6.0;
                row[C + 14] = g.height[s] as f32 / 2.0;
                row[C + 47] = (s == self.pad) as u8 as f32;
                push(self, Cand { kind: Kind::Gather, a: s, b: s, troops: 0 }, &mut row);
            }
        }

        for k in C..F {
            row[k] = 0.0;
        }
        push(self, Cand { kind: Kind::NoOp, a: 0, b: 0, troops: 0 }, &mut row);
    }

    /// Wall links for every adjacent pair of our Walls that both have a free link.
    pub fn wall_links(&self, view: &View) -> Vec<ManagementCommand> {
        let max = self.rules.wall.max_links as usize;
        let mut walls: Vec<(Hex, usize, Vec<Hex>)> = view
            .visible_hexes
            .iter()
            .filter(|c| c.owner == Some(view.you))
            .filter_map(|c| {
                let b = c.building.as_ref()?;
                (b.kind == Building::Wall && b.link_cooldown == 0).then(|| (c.hex, b.links.len(), b.links.clone()))
            })
            .collect();
        let mut out = vec![];
        for i in 0..walls.len() {
            for j in i + 1..walls.len() {
                if walls[i].1 >= max {
                    break;
                }
                if walls[j].1 >= max || walls[i].0.distance(walls[j].0) != 1 || walls[i].2.contains(&walls[j].0) {
                    continue;
                }
                out.push(ManagementCommand::SetWallLink { wall_a: walls[i].0, wall_b: walls[j].0, enabled: true });
                walls[i].1 = max;
                walls[j].1 = max;
            }
        }
        out
    }
}

pub struct Policy {
    name: String,
    pub fz: Featurizer,
    model: Model,
    folded: Folded,
    pub hist: History,
    scores: Vec<f32>,
    /// Sampling temperature (0 = argmax) and its random state, for exploration.
    pub temperature: f32,
    rng: u64,
}

impl Policy {
    pub fn new(style: Style) -> Policy {
        Self::with_model(style.id, Model::embedded())
    }

    pub fn with_model(name: String, model: Model) -> Policy {
        Policy {
            name,
            fz: Featurizer::new(),
            folded: Folded::new(&model),
            model,
            hist: History::default(),
            scores: vec![],
            temperature: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    /// This tick's candidate scores, after [`Policy::choose`].
    pub fn scores(&self) -> &[f32] {
        &self.scores
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn set_model(&mut self, model: Model) {
        self.folded = Folded::new(&model);
        self.model = model;
    }

    pub fn seed(&mut self, s: u64) {
        self.rng = s | 1;
    }

    fn next_f32(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Index of the chosen candidate.
    pub fn choose(&mut self) -> usize {
        let mut scores = std::mem::take(&mut self.scores);
        self.folded.score_all(&self.fz.feats, &mut scores);
        let mut best = 0;
        for (i, &s) in scores.iter().enumerate() {
            if s > scores[best] {
                best = i;
            }
        }
        if self.temperature > 0.0 {
            let t = self.temperature;
            let m = scores[best];
            let total: f32 = scores.iter().map(|&s| ((s - m) / t).exp()).sum();
            let mut u = self.next_f32() * total;
            for (i, &s) in scores.iter().enumerate() {
                u -= ((s - m) / t).exp();
                if u <= 0.0 {
                    best = i;
                    break;
                }
            }
        }
        self.scores = scores;
        best
    }
}

impl Bot for Policy {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        self.fz.build(view, &self.hist);
        let i = self.choose();
        let c = self.fz.cands[i];
        let action = self.fz.action(&c);
        let captured = matches!(c.kind, Kind::CapEnemy | Kind::CapNeutral);
        self.hist.record(view.tick, &action, captured);
        PlayerCommand { management: self.fz.wall_links(view), action }
    }
}
