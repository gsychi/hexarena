//! Nets: multi-capture rings found as minimum vertex cuts.
//!
//! [`super::rings`] prices the one capture that seals a region off from the board edge.
//! A net is the cheapest *set* of captures that would: the minimum vertex cut between a
//! seed hex and the board edge through the land we don't own, every passable hex costing
//! one capture (one tick's action) and impassable hexes uncuttable. Unit-capacity
//! augmenting paths stop as soon as the cut would need more than `k` captures, so a pass
//! costs at most `k + 1` breadth-first searches. Of all minimum cuts the one farthest from
//! the seed is taken (everything that can't reach the edge in the residual graph), so the
//! region it seals is as large as the cut allows.

use super::rings::{Board, Loot, NONE};

const INF: i32 = i32::MAX / 4;

/// What one net would take.
pub struct Plan {
    /// Hexes to capture, in slot order.
    pub cut: Vec<u32>,
    /// What the regions it seals hold (the cut itself excluded).
    pub region: Loot,
}

#[derive(Default)]
pub struct Net {
    /// Index of `v` in the adjacency of its `k`-th neighbour.
    rev: Vec<[u8; 6]>,
    /// Flow through each hex (in-node to out-node).
    through: Vec<i32>,
    /// Net flow from each hex to its `k`-th neighbour.
    flow: Vec<[i32; 6]>,
    parent: Vec<u32>,
    queue: Vec<u32>,
    reach: Vec<bool>,
    /// Hexes of the last plan's regions.
    pub inside: Vec<u32>,
    /// Search nodes expanded since the caller last reset it, to cap a tick's work.
    pub work: u64,
}

impl Net {
    fn prepare(&mut self, b: &Board) {
        if self.rev.len() == b.slots {
            return;
        }
        self.rev = vec![[0; 6]; b.slots];
        for v in 0..b.slots {
            for k in 0..6 {
                let u = b.adj[v][k];
                if u == NONE {
                    continue;
                }
                let kk = b.adj[u as usize].iter().position(|&x| x == v as u32).unwrap_or(0);
                self.rev[v][k] = kk as u8;
            }
        }
    }

    /// The farthest minimum net (at most `k` captures) cutting `seed` off from the board
    /// edge through hexes `blocked` doesn't hold, with the loot of what it seals, or
    /// `None` when it needs more than `k` captures or `seed` is already cut off.
    pub fn plan(&mut self, b: &Board, blocked: &[bool], w: &[Loot], seed: usize, k: u32) -> Option<Plan> {
        self.prepare(b);
        let slots = b.slots;
        let n = 2 * slots + 1;
        let sink = (2 * slots) as u32;
        let open = |v: usize| b.valid[v] && !blocked[v];
        let cap = |v: usize| if v == seed || !b.passable[v] { INF } else { 1 };
        self.through.clear();
        self.through.resize(slots, 0);
        self.flow.clear();
        self.flow.resize(slots, [0; 6]);
        let source = (2 * seed + 1) as u32;
        let mut total = 0u32;
        loop {
            self.parent.clear();
            self.parent.resize(n, NONE);
            self.parent[source as usize] = source;
            self.queue.clear();
            self.queue.push(source);
            let mut qi = 0;
            let mut found = false;
            'bfs: while qi < self.queue.len() {
                let x = self.queue[qi];
                qi += 1;
                let v = (x / 2) as usize;
                let push = |y: u32, parent: &mut Vec<u32>, queue: &mut Vec<u32>| {
                    if parent[y as usize] == NONE {
                        parent[y as usize] = x;
                        queue.push(y);
                    }
                };
                if x % 2 == 0 {
                    if self.through[v] < cap(v) {
                        push(x + 1, &mut self.parent, &mut self.queue);
                    }
                    for kk in 0..6 {
                        let u = b.adj[v][kk];
                        if u != NONE && open(u as usize) && self.flow[v][kk] < 0 {
                            push(2 * u + 1, &mut self.parent, &mut self.queue);
                        }
                    }
                } else {
                    if b.boundary[v] {
                        self.parent[sink as usize] = x;
                        found = true;
                        break 'bfs;
                    }
                    if self.through[v] > 0 {
                        push(x - 1, &mut self.parent, &mut self.queue);
                    }
                    for kk in 0..6 {
                        let u = b.adj[v][kk];
                        if u != NONE && open(u as usize) {
                            push(2 * u, &mut self.parent, &mut self.queue);
                        }
                    }
                }
            }
            self.work += qi as u64;
            if !found {
                break;
            }
            total += 1;
            if total > k {
                return None;
            }
            let mut y = sink;
            while y != source {
                let p = self.parent[y as usize];
                let (pv, yv) = ((p / 2) as usize, (y / 2) as usize);
                if y == sink {
                } else if pv == yv {
                    self.through[pv] += if p % 2 == 0 { 1 } else { -1 };
                } else {
                    let kk = b.adj[pv].iter().position(|&u| u == yv as u32).unwrap();
                    self.flow[pv][kk] += 1;
                    self.flow[yv][self.rev[pv][kk] as usize] -= 1;
                }
                y = p;
            }
        }
        if total == 0 {
            return None;
        }
        // Nodes that can still reach the sink in the residual graph.
        self.reach.clear();
        self.reach.resize(n, false);
        self.queue.clear();
        for v in 0..slots {
            if open(v) && b.boundary[v] {
                self.reach[2 * v + 1] = true;
                self.queue.push((2 * v + 1) as u32);
            }
        }
        let mut qi = 0;
        while qi < self.queue.len() {
            let y = self.queue[qi];
            qi += 1;
            let v = (y / 2) as usize;
            let mark = |x: usize, reach: &mut Vec<bool>, queue: &mut Vec<u32>| {
                if !reach[x] {
                    reach[x] = true;
                    queue.push(x as u32);
                }
            };
            if y % 2 == 0 {
                // Into in(v): from out(u) of any open neighbour, and from out(v) if flow runs through v.
                for kk in 0..6 {
                    let u = b.adj[v][kk];
                    if u != NONE && open(u as usize) {
                        mark(2 * u as usize + 1, &mut self.reach, &mut self.queue);
                    }
                }
                if self.through[v] > 0 {
                    mark(2 * v + 1, &mut self.reach, &mut self.queue);
                }
            } else {
                // Into out(v): from in(v) with spare capacity, from in(u) where flow runs v -> u.
                if self.through[v] < cap(v) {
                    mark(2 * v, &mut self.reach, &mut self.queue);
                }
                for kk in 0..6 {
                    let u = b.adj[v][kk];
                    if u != NONE && open(u as usize) && self.flow[v][kk] > 0 {
                        mark(2 * u as usize, &mut self.reach, &mut self.queue);
                    }
                }
            }
        }
        self.work += qi as u64;
        let mut cut = vec![];
        let mut region = Loot::default();
        self.inside.clear();
        for v in 0..slots {
            if !open(v) {
                continue;
            }
            let (i, o) = (self.reach[2 * v], self.reach[2 * v + 1]);
            if !i && o {
                cut.push(v as u32);
            } else if !i && !o {
                region.add(&w[v]);
                self.inside.push(v as u32);
            }
        }
        Some(Plan { cut, region })
    }
}

/// Dinic max-flow on a small explicit graph, reused between ticks.
#[derive(Default)]
struct Dinic {
    head: Vec<u32>,
    next: Vec<u32>,
    to: Vec<u32>,
    cap: Vec<i64>,
    level: Vec<i32>,
    iter: Vec<u32>,
    queue: Vec<u32>,
}

impl Dinic {
    fn reset(&mut self, n: usize) {
        self.head.clear();
        self.head.resize(n, NONE);
        self.next.clear();
        self.to.clear();
        self.cap.clear();
    }

    fn edge(&mut self, a: usize, b: usize, c: i64) {
        for (x, y, cc) in [(a, b, c), (b, a, 0)] {
            self.to.push(y as u32);
            self.cap.push(cc);
            self.next.push(self.head[x]);
            self.head[x] = (self.to.len() - 1) as u32;
        }
    }

    fn bfs(&mut self, s: usize, t: usize) -> bool {
        self.level.clear();
        self.level.resize(self.head.len(), -1);
        self.level[s] = 0;
        self.queue.clear();
        self.queue.push(s as u32);
        let mut qi = 0;
        while qi < self.queue.len() {
            let v = self.queue[qi] as usize;
            qi += 1;
            let mut e = self.head[v];
            while e != NONE {
                let u = self.to[e as usize] as usize;
                if self.cap[e as usize] > 0 && self.level[u] < 0 {
                    self.level[u] = self.level[v] + 1;
                    self.queue.push(u as u32);
                }
                e = self.next[e as usize];
            }
        }
        self.level[t] >= 0
    }

    fn dfs(&mut self, v: usize, t: usize, f: i64) -> i64 {
        if v == t {
            return f;
        }
        while self.iter[v] != NONE {
            let e = self.iter[v] as usize;
            let u = self.to[e] as usize;
            if self.cap[e] > 0 && self.level[u] == self.level[v] + 1 {
                let d = self.dfs(u, t, f.min(self.cap[e]));
                if d > 0 {
                    self.cap[e] -= d;
                    self.cap[e ^ 1] += d;
                    return d;
                }
            }
            self.iter[v] = self.next[e];
        }
        0
    }

    /// Max flow from `s` to `t`, giving up after `rounds` phases. Returns the work done.
    fn run(&mut self, s: usize, t: usize, rounds: u32) -> u64 {
        let mut work = 0u64;
        for _ in 0..rounds {
            if !self.bfs(s, t) {
                break;
            }
            work += self.queue.len() as u64;
            self.iter.clone_from(&self.head);
            loop {
                let f = self.dfs(s, t, i64::MAX / 4);
                if f == 0 {
                    break;
                }
                work += 8;
            }
        }
        work
    }

    /// Nodes reachable from `s` in the residual graph.
    fn source_side(&mut self, s: usize, out: &mut Vec<bool>) {
        out.clear();
        out.resize(self.head.len(), false);
        out[s] = true;
        self.queue.clear();
        self.queue.push(s as u32);
        let mut qi = 0;
        while qi < self.queue.len() {
            let v = self.queue[qi] as usize;
            qi += 1;
            let mut e = self.head[v];
            while e != NONE {
                let u = self.to[e as usize] as usize;
                if self.cap[e as usize] > 0 && !out[u] {
                    out[u] = true;
                    self.queue.push(u as u32);
                }
                e = self.next[e as usize];
            }
        }
    }
}

/// Giant nets: the set of regions near our land that best pays for the captures
/// sealing them. Every hex we don't own within `reach` steps of our land may end up
/// inside (worth its value) or on a net (costing `price` per capture; impassable hexes
/// can't be captured); a hex on the board edge or next to land beyond `reach` can't be
/// inside. One min cut picks the regions maximizing value inside minus nets' price.
#[derive(Default)]
pub struct Closure {
    flow: Dinic,
    node: Vec<u32>,
    dist: Vec<u8>,
    side: Vec<bool>,
    comp: Vec<u32>,
    pub work: u64,
}

/// One net from [`Closure::solve`].
pub struct Catch {
    pub cut: Vec<u32>,
    pub region: Loot,
}

impl Closure {
    /// `value[v]`: what hex `v` is worth inside a region (same units as `price`).
    pub fn solve(
        &mut self,
        b: &Board,
        mine: &[bool],
        w: &[Loot],
        value: &[i64],
        price: i64,
        reach: u8,
        rounds: u32,
    ) -> Vec<Catch> {
        let slots = b.slots;
        self.dist.clear();
        self.dist.resize(slots, u8::MAX);
        let mut queue: Vec<u32> = (0..slots).filter(|&i| mine[i]).map(|i| i as u32).collect();
        for &i in &queue {
            self.dist[i as usize] = 0;
        }
        let mut qi = 0;
        while qi < queue.len() {
            let v = queue[qi] as usize;
            qi += 1;
            if self.dist[v] >= reach {
                continue;
            }
            for &u in &b.adj[v] {
                if u != NONE && b.valid[u as usize] && self.dist[u as usize] == u8::MAX {
                    self.dist[u as usize] = self.dist[v] + 1;
                    queue.push(u);
                }
            }
        }
        self.node.clear();
        self.node.resize(slots, NONE);
        let mut nodes = 0usize;
        for v in 0..slots {
            if b.valid[v] && !mine[v] && self.dist[v] != u8::MAX {
                self.node[v] = nodes as u32;
                nodes += 1;
            }
        }
        let (s, t) = (2 * nodes, 2 * nodes + 1);
        self.flow.reset(2 * nodes + 2);
        const INF: i64 = i64::MAX / 8;
        for v in 0..slots {
            let x = self.node[v];
            if x == NONE {
                continue;
            }
            let (i, o) = (2 * x as usize, 2 * x as usize + 1);
            if value[v] > 0 {
                self.flow.edge(s, i, value[v]);
            }
            self.flow.edge(i, o, if b.passable[v] { price } else { INF });
            let mut outside = b.boundary[v];
            for &u in &b.adj[v] {
                if u == NONE || mine[u as usize] {
                    continue;
                }
                let y = self.node[u as usize];
                if y == NONE {
                    outside = true;
                } else {
                    self.flow.edge(o, 2 * y as usize, INF);
                }
            }
            if outside {
                self.flow.edge(o, t, INF);
            }
        }
        self.work = self.flow.run(s, t, rounds);
        self.flow.source_side(s, &mut self.side);
        if self.side[t] {
            return vec![];
        }
        // Group nets and the regions they seal into connected pieces.
        let mut seen = vec![false; slots];
        let mut out = vec![];
        for v0 in 0..slots {
            let x = self.node[v0];
            if x == NONE || seen[v0] || !self.side[2 * x as usize] {
                continue;
            }
            self.comp.clear();
            self.comp.push(v0 as u32);
            seen[v0] = true;
            let mut k = 0;
            while k < self.comp.len() {
                let v = self.comp[k] as usize;
                k += 1;
                for &u in &b.adj[v] {
                    if u == NONE || seen[u as usize] {
                        continue;
                    }
                    let y = self.node[u as usize];
                    if y != NONE && self.side[2 * y as usize] {
                        seen[u as usize] = true;
                        self.comp.push(u);
                    }
                }
            }
            let mut c = Catch { cut: vec![], region: Loot::default() };
            for &v in &self.comp {
                let y = self.node[v as usize] as usize;
                if self.side[2 * y + 1] {
                    c.region.add(&w[v as usize]);
                } else {
                    c.cut.push(v);
                }
            }
            if !c.cut.is_empty() && c.region.hexes > 0 {
                out.push(c);
            }
        }
        out
    }
}
