//! Exact Go-style region analysis (rules v0.7) in one linear pass.
//!
//! Taking hex `t` hands the capturer every region next to `t` that consists of hexes it
//! doesn't own and no longer touches the board edge. Treat every hex the capturer
//! doesn't own (impassable included) as a node, add one virtual node joined to every
//! hex on the board edge, and root a depth-first search there: the regions taking `t`
//! seals off are exactly the DFS subtrees below `t` that have no back edge above `t`,
//! i.e. `t` is an articulation point separating them from the edge. One Tarjan pass
//! therefore prices every hex at once, and each node carries a few [`Loot`] channels
//! (hexes, enemy hexes, troops, buildings...) summed over the subtrees it would seal.
//! Ownership of the hexes inside doesn't matter for *whether* a region falls, so fog
//! only blurs what it is worth, never the geometry.

use crate::{Hex, View};

pub const NONE: u32 = u32::MAX;

/// What a sealed-off region holds, summed over its hexes.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Loot {
    /// Every hex, impassable included (bounds floods).
    pub nodes: u32,
    /// Passable hexes: land that changes hands.
    pub hexes: u32,
    /// Hexes some rival owns.
    pub enemy: u32,
    /// Hexes the observer owns (for threats against us).
    pub mine: u32,
    /// Troops standing on the region's hexes.
    pub troops: u32,
    /// Villages in it.
    pub villages: u32,
    /// Farms and Quarries in it.
    pub farms: u32,
}

impl Loot {
    pub fn add(&mut self, o: &Loot) {
        self.nodes += o.nodes;
        self.hexes += o.hexes;
        self.enemy += o.enemy;
        self.mine += o.mine;
        self.troops = self.troops.saturating_add(o.troops);
        self.villages += o.villages;
        self.farms += o.farms;
    }

    fn sub(&mut self, o: &Loot) {
        self.nodes -= o.nodes;
        self.hexes -= o.hexes;
        self.enemy -= o.enemy;
        self.mine -= o.mine;
        self.troops = self.troops.saturating_sub(o.troops);
        self.villages -= o.villages;
        self.farms -= o.farms;
    }
}

/// Static board shape: every on-board slot, its six on-board neighbours and whether it
/// sits on the board edge.
pub struct Board {
    pub slots: usize,
    pub valid: Vec<bool>,
    pub passable: Vec<bool>,
    pub adj: Vec<[u32; 6]>,
    pub boundary: Vec<bool>,
    boundary_list: Vec<u32>,
}

impl Board {
    pub fn new(view: &View) -> Board {
        let r = view.radius;
        let w = 2 * r + 1;
        let slots = (w * w) as usize;
        let slot = |h: Hex| {
            (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
                .then(|| ((h.r + r) * w + h.q + r) as u32)
        };
        let mut valid = vec![false; slots];
        let mut passable = vec![false; slots];
        let mut adj = vec![[NONE; 6]; slots];
        let mut boundary = vec![false; slots];
        let mut boundary_list = vec![];
        for i in 0..slots {
            let h = Hex::new(i as i32 % w - r, i as i32 / w - r);
            if slot(h).is_none() {
                continue;
            }
            valid[i] = true;
            passable[i] = view.public_terrain[i].is_some_and(|t| t.passable());
            for (k, n) in h.neighbors().into_iter().enumerate() {
                match slot(n) {
                    Some(j) => adj[i][k] = j,
                    None => boundary[i] = true,
                }
            }
        }
        // Impassable hexes joined to the board edge through impassable hexes can never be
        // owned, so every region touching one touches the edge: fold them into the edge
        // node. Exact, and on the real-world maps it drops most of the sea from every pass.
        let wall = |i: usize| view.public_terrain[i].is_some_and(|t| !t.passable());
        let mut sea = vec![false; slots];
        let mut queue: Vec<usize> = (0..slots).filter(|&i| valid[i] && boundary[i] && wall(i)).collect();
        for &i in &queue {
            sea[i] = true;
        }
        while let Some(c) = queue.pop() {
            for &j in &adj[c] {
                if j != NONE && !sea[j as usize] && wall(j as usize) {
                    sea[j as usize] = true;
                    queue.push(j as usize);
                }
            }
        }
        for i in 0..slots {
            if sea[i] {
                valid[i] = false;
                boundary[i] = false;
                adj[i] = [NONE; 6];
                continue;
            }
            for k in 0..6 {
                if adj[i][k] != NONE && sea[adj[i][k] as usize] {
                    adj[i][k] = NONE;
                    boundary[i] = true;
                }
            }
            if valid[i] && boundary[i] {
                boundary_list.push(i as u32);
            }
        }
        Board { slots, valid, passable, adj, boundary, boundary_list }
    }
}

/// Reusable scratch for [`Tarjan::run`], so a tick allocates nothing.
#[derive(Default)]
pub struct Tarjan {
    disc: Vec<u32>,
    low: Vec<u32>,
    parent: Vec<u32>,
    sub: Vec<Loot>,
    stack: Vec<(u32, u32)>,
    comp: Vec<u32>,
    mark: Vec<u32>,
    gen: u32,
    queue: Vec<u32>,
}

impl Tarjan {
    /// `out[t]`: the loot of every region taking `t` would seal off, for the capturer
    /// whose land is `blocked`. Zero for blocked hexes.
    pub fn run(&mut self, b: &Board, blocked: &[bool], w: &[Loot], out: &mut Vec<Loot>) {
        let n = b.slots + 1;
        let edge = b.slots as u32;
        self.disc.clear();
        self.disc.resize(n, 0);
        self.low.resize(n, 0);
        self.parent.resize(n, NONE);
        self.sub.resize(n, Loot::default());
        out.clear();
        out.resize(b.slots, Loot::default());
        let open = |j: u32| !blocked[j as usize];
        let mut timer = 1u32;
        self.disc[edge as usize] = 1;
        self.low[edge as usize] = 1;
        self.sub[edge as usize] = Loot::default();
        self.parent[edge as usize] = NONE;
        self.stack.clear();
        self.stack.push((edge, 0));
        while let Some(&(v, it)) = self.stack.last() {
            // Next open neighbour of v from position `it`.
            let mut next = None;
            let mut k = it;
            if v == edge {
                while (k as usize) < b.boundary_list.len() {
                    let j = b.boundary_list[k as usize];
                    k += 1;
                    if open(j) {
                        next = Some(j);
                        break;
                    }
                }
            } else {
                let vi = v as usize;
                while k < 7 {
                    let j = if k < 6 { b.adj[vi][k as usize] } else if b.boundary[vi] { edge } else { NONE };
                    k += 1;
                    if j != NONE && (j == edge || open(j)) {
                        next = Some(j);
                        break;
                    }
                }
            }
            self.stack.last_mut().unwrap().1 = k;
            match next {
                Some(j) => {
                    let ju = j as usize;
                    if self.disc[ju] == 0 {
                        timer += 1;
                        self.disc[ju] = timer;
                        self.low[ju] = timer;
                        self.parent[ju] = v;
                        self.sub[ju] = w[ju];
                        self.stack.push((j, 0));
                    } else if j != self.parent[v as usize] {
                        let d = self.disc[ju];
                        let lv = &mut self.low[v as usize];
                        *lv = (*lv).min(d);
                    }
                }
                None => {
                    self.stack.pop();
                    if v == edge {
                        continue;
                    }
                    let p = self.parent[v as usize] as usize;
                    let (lv, sv) = (self.low[v as usize], self.sub[v as usize]);
                    self.low[p] = self.low[p].min(lv);
                    self.sub[p].add(&sv);
                    if p != edge as usize && lv >= self.disc[p] {
                        out[p].add(&sv);
                    }
                }
            }
        }
        // Components the edge can't reach are already sealed: taking any hex in one
        // hands over the rest of it.
        for u in 0..b.slots {
            if !b.valid[u] || blocked[u] || self.disc[u] != 0 {
                continue;
            }
            self.comp.clear();
            self.comp.push(u as u32);
            self.disc[u] = 1;
            let mut total = Loot::default();
            let mut i = 0;
            while i < self.comp.len() {
                let c = self.comp[i] as usize;
                i += 1;
                total.add(&w[c]);
                for &j in &b.adj[c] {
                    if j != NONE && open(j) && self.disc[j as usize] == 0 {
                        self.disc[j as usize] = 1;
                        self.comp.push(j);
                    }
                }
            }
            for &c in &self.comp {
                let mut l = total;
                l.sub(&w[c as usize]);
                out[c as usize] = l;
            }
        }
    }

    /// The region the last [`Self::sealed`] call that returned true flooded.
    pub fn flooded(&self) -> &[u32] {
        &self.queue
    }

    /// Whether the open neighbour `n` of `t` falls with it: a flood from `n` that avoids
    /// `t` and blocked hexes stays off the board edge within `bound` hexes.
    pub fn sealed(&mut self, b: &Board, blocked: &[bool], t: usize, n: usize, bound: u32) -> bool {
        if self.mark.len() != b.slots {
            self.mark = vec![0; b.slots];
            self.gen = 0;
        }
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            self.mark.iter_mut().for_each(|m| *m = 0);
            self.gen = 1;
        }
        let g = self.gen;
        self.queue.clear();
        self.queue.push(n as u32);
        self.mark[n] = g;
        self.mark[t] = g;
        let mut i = 0;
        while i < self.queue.len() {
            let c = self.queue[i] as usize;
            i += 1;
            if b.boundary[c] || self.queue.len() as u32 > bound {
                return false;
            }
            for &j in &b.adj[c] {
                if j != NONE && !blocked[j as usize] && self.mark[j as usize] != g {
                    self.mark[j as usize] = g;
                    self.queue.push(j);
                }
            }
        }
        true
    }
}
