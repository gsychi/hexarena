//! Ridgeline: a border-holding land racer.
//!
//! Late in a free-for-all every seat is limited by its one action a tick, not by troops,
//! so land is decided by how many captured hexes stay captured. Ridgeline therefore
//! garrisons every hex it takes from the surplus of the hex it attacks from, never sends
//! a capture that could plausibly bounce, and prefers ground that shortens its exposed
//! border: hexes hugged by its own land, hexes shielded by mountains, water and the map
//! edge, high ground that attackers must climb, and captures that make an enemy pocket
//! surrender.

use crate::{Action, Bot, Hex, PlayerCommand, PlayerId, RulesConfig, Style, View, MILLI};

/// Map-static data, built from the first view.
struct Geometry {
    radius: i32,
    width: i32,
    passable: Vec<bool>,
    height: Vec<i8>,
    /// Passable neighbour slots of each slot.
    nbrs: Vec<Vec<usize>>,
    hexes: Vec<Hex>,
}

impl Geometry {
    fn new(view: &View) -> Self {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let mut passable = vec![false; slots];
        let mut height = vec![0i8; slots];
        let mut hexes = vec![Hex::new(0, 0); slots];
        for i in 0..slots {
            let h = Hex::new(i as i32 % width - radius, i as i32 / width - radius);
            hexes[i] = h;
            if let Some(t) = view.public_terrain[i] {
                passable[i] = t.passable();
                height[i] = t.height;
            }
        }
        let mut g = Geometry { radius, width, passable, height, nbrs: vec![], hexes };
        g.nbrs = (0..slots)
            .map(|i| {
                g.hexes[i]
                    .neighbors()
                    .iter()
                    .filter_map(|n| g.slot(*n))
                    .filter(|&j| g.passable[j])
                    .collect()
            })
            .collect();
        g
    }

    fn slot(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * self.width + h.q + r) as usize)
    }
}

#[derive(Clone, Copy)]
struct Cell {
    owner: Option<PlayerId>,
    troops: u32,
    wall_milli: u32,
}

/// One affordable capture under consideration.
#[derive(Clone, Copy)]
struct Capture {
    score: i64,
    from: usize,
    to: usize,
    send: u32,
}

pub struct Ridgeline {
    name: String,
    rules: RulesConfig,
    geo: Option<Geometry>,
    /// Last seen state of every slot.
    seen: Vec<Option<Cell>>,
    sure_x10: i64,
    push_pct: i64,
    land: i64,
    enemy: i64,
    hug: i64,
    open: i64,
    height: i64,
    cost_pct: i64,
    enclose: i64,
    gather_until: u32,
    keep_k: i64,
    keep_w: i64,
}

impl Ridgeline {
    pub fn new(style: Style) -> Self {
        let k = |n: &str, d: i64| style.params.knob(n, d);
        Ridgeline {
            name: style.id.clone(),
            rules: RulesConfig::default(),
            geo: None,
            seen: vec![],
            sure_x10: k("sure_x10", 30),
            push_pct: k("push_pct", 60),
            land: k("land", 100),
            enemy: k("enemy", 30),
            hug: k("hug", 25),
            open: k("open", 15),
            height: k("height", 10),
            cost_pct: k("cost_pct", 100),
            enclose: k("enclose", 100),
            gather_until: k("gather_until", 400) as u32,
            keep_k: k("keep_k", 40),
            keep_w: k("keep_w", 100),
        }
    }

    /// Mean and standard deviation (both x1000) of the attackers lost taking `def`
    /// troops behind `defense` milli, from the engine's exchange loop.
    fn loss_stats(&self, def: u32, defense: u32) -> (u64, u64) {
        let d = defense.max(1) as u64;
        let per_win = (1_000_000 + d / 2) / d;
        let wins = (def as u64 * 1000).div_ceil(per_win);
        let p = self.rules.attacker_exchange_win_milli as u64;
        let q = 1000 - p;
        let mean = wins * q * 1000 / p;
        // Negative binomial: var = wins * q / p^2.
        let var_x1e6 = wins * q * 1_000_000_000 / (p * p);
        (mean, isqrt(var_x1e6))
    }

    fn owned(&self, i: usize, me: PlayerId) -> bool {
        self.seen[i].is_some_and(|c| c.owner == Some(me))
    }

    /// Hexes of the enemy pockets that would surrender if we took `to`.
    fn enclosure_gain(&self, geo: &Geometry, to: usize, me: PlayerId) -> i64 {
        const LIMIT: usize = 64;
        let mut gain = 0i64;
        let mut done: Vec<usize> = vec![];
        for nb in geo.hexes[to].neighbors() {
            let Some(n) = geo.slot(nb) else { continue };
            let Some(owner) = self.seen[n].and_then(|c| c.owner) else { continue };
            if owner == me || done.contains(&n) {
                continue;
            }
            let mut comp = vec![n];
            let mut k = 0;
            let mut enclosed = true;
            while k < comp.len() && enclosed {
                let c = comp[k];
                k += 1;
                for h in geo.hexes[c].neighbors() {
                    let Some(j) = geo.slot(h).filter(|&j| geo.passable[j]) else {
                        enclosed = false;
                        break;
                    };
                    if j == to {
                        continue;
                    }
                    match self.seen[j].and_then(|c| c.owner) {
                        Some(o) if o == owner => {
                            if !comp.contains(&j) {
                                comp.push(j);
                            }
                        }
                        Some(o) if o == me => {}
                        _ => {
                            enclosed = false;
                            break;
                        }
                    }
                }
                if comp.len() > LIMIT {
                    enclosed = false;
                }
            }
            done.extend(&comp);
            if enclosed {
                gain += comp.len() as i64;
            }
        }
        gain
    }

    /// Cheapest height multiplier (milli) any enemy neighbour of `h` attacks it with,
    /// counting `ours` as already ours; `None` when no enemy touches `h`.
    fn threat_mult(&self, geo: &Geometry, me: PlayerId, h: usize, ours: usize) -> Option<u64> {
        geo.nbrs[h]
            .iter()
            .filter(|&&n| n != ours)
            .filter(|&&n| self.seen[n].is_some_and(|c| c.owner.is_some_and(|o| o != me)))
            .map(|&n| self.rules.height_multiplier_milli(geo.height[n], geo.height[h]) as u64)
            .min()
    }

    /// Chance (milli) a hex holding `troops` behind `mult` is left alone.
    fn keep(&self, troops: u64, mult: Option<u64>) -> i64 {
        match mult {
            None => 1000,
            Some(m) => {
                let x = troops * m / 1000;
                (x * 1000 / (x + self.keep_k as u64)) as i64
            }
        }
    }

    /// How many of the source's `troops` to send so that both the captured hex and the
    /// source stay unattractive to enemies. Returns the send and the net change in
    /// held hexes (milli) it buys.
    fn split(
        &self,
        geo: &Geometry,
        me: PlayerId,
        s: usize,
        t: usize,
        troops: u64,
        loss: u64,
        need: u64,
    ) -> (u64, i64) {
        let avail = troops - 1;
        let mt = self.threat_mult(geo, me, t, s);
        let ms_before = self.threat_mult(geo, me, s, usize::MAX);
        let ms = self.threat_mult(geo, me, s, t);
        let before = self.keep(troops, ms_before);
        let mut cands = [need, avail, 0, 0, 0, 0];
        for (k, f) in [1u64, 2, 3].iter().enumerate() {
            cands[2 + k] = need + (avail - need) * f / 4;
        }
        if let (Some(a), Some(b)) = (mt, ms) {
            cands[5] = ((troops * b + loss * a) / (a + b)).clamp(need, avail);
        } else {
            cands[5] = avail;
        }
        let mut best = (i64::MIN, avail);
        for &send in &cands {
            let v = self.keep(send.saturating_sub(loss), mt) + self.keep(troops - send, ms) - before;
            if v > best.0 || (v == best.0 && send > best.1) {
                best = (v, send);
            }
        }
        (best.1, best.0)
    }

    fn best_capture(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<Capture> {
        let mut best: Option<Capture> = None;
        for &s in mine {
            let src = self.seen[s].unwrap();
            if src.troops < 2 {
                continue;
            }
            let avail = src.troops as u64 - 1;
            let sh = geo.height[s];
            for &t in &geo.nbrs[s] {
                let Some(tc) = self.seen[t] else { continue };
                if tc.owner == Some(me) {
                    continue;
                }
                let wall = (tc.wall_milli != MILLI).then_some(tc.wall_milli);
                let defense = self
                    .rules
                    .height_multiplier_milli(sh, geo.height[t]) as u64
                    * wall.unwrap_or(MILLI) as u64
                    / 1000;
                let defense = defense.min(self.rules.maximum_total_defense_milli as u64) as u32;
                let (mean, sd) = self.loss_stats(tc.troops, defense);
                let need = (mean + sd * self.sure_x10 as u64 / 10).div_ceil(1000) + 1;
                if need > avail {
                    continue;
                }
                let loss = mean.div_ceil(1000);
                let (send, keep) = if self.keep_k > 0 {
                    self.split(geo, me, s, t, src.troops as u64, loss, need)
                } else {
                    // Does the source keep facing land we don't own once `t` is ours?
                    let src_exposed = geo.nbrs[s].iter().any(|&n| n != t && !self.owned(n, me));
                    let send = if src_exposed {
                        let surplus = avail.saturating_sub(loss);
                        (loss + surplus * self.push_pct as u64 / 100).clamp(need, avail)
                    } else {
                        avail
                    };
                    (send, 0)
                };
                let hugged = geo.nbrs[t].iter().filter(|&&n| self.owned(n, me)).count() as i64;
                let exposed = geo.nbrs[t].len() as i64 - hugged;
                let mut score = self.land + self.hug * hugged - self.open * exposed
                    + self.height * geo.height[t] as i64
                    - loss as i64 * self.cost_pct / 100
                    + keep * self.keep_w / 1000;
                if tc.owner.is_some() {
                    score += self.enemy + self.enclose * self.enclosure_gain(geo, t, me);
                }
                let better = best.as_ref().map_or(true, |b| {
                    (score, std::cmp::Reverse(t), send) > (b.score, std::cmp::Reverse(b.to), b.send as u64)
                });
                if better {
                    best = Some(Capture { score, from: s, to: t, send: send as u32 });
                }
            }
        }
        best
    }

    /// Nothing affordable: pull troops toward the border hex facing the cheapest target.
    fn regroup(&self, geo: &Geometry, me: PlayerId, mine: &[usize], tick: u32) -> Option<Action> {
        if mine.len() < 2 {
            return None;
        }
        let mut cheapest: Option<(u32, usize)> = None;
        for &s in mine {
            for &t in &geo.nbrs[s] {
                let Some(tc) = self.seen[t].filter(|c| c.owner != Some(me)) else { continue };
                let key = (tc.troops, s);
                if cheapest.map_or(true, |c| key < c) {
                    cheapest = Some(key);
                }
            }
        }
        let (_, at) = cheapest?;
        if tick < self.gather_until {
            return Some(Action::Gather { target: geo.hexes[at] });
        }
        // Later on a Gather strips every far border hex down to one troop, so instead
        // walk the biggest neighbouring stack onto the target's border hex.
        let from = geo.nbrs[at]
            .iter()
            .copied()
            .filter(|&n| self.owned(n, me))
            .max_by_key(|&n| (self.seen[n].unwrap().troops, std::cmp::Reverse(n)))?;
        let troops = self.seen[from].unwrap().troops;
        (troops >= 2).then(|| Action::Move {
            from: geo.hexes[from],
            to: geo.hexes[at],
            troops: troops - 1,
        })
    }
}

fn isqrt(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as u64;
    while x * x > v {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= v {
        x += 1;
    }
    x
}

impl Bot for Ridgeline {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        let geo = self.geo.take().unwrap_or_else(|| Geometry::new(view));
        if self.seen.is_empty() {
            self.seen = vec![None; geo.passable.len()];
        }
        let me = view.you;
        let mut mine = vec![];
        for c in &view.visible_hexes {
            let Some(i) = geo.slot(c.hex) else { continue };
            self.seen[i] = Some(Cell { owner: c.owner, troops: c.troops, wall_milli: c.wall_milli });
            if c.owner == Some(me) {
                mine.push(i);
            }
        }
        let action = match self.best_capture(&geo, me, &mine) {
            Some(c) => Action::Move { from: geo.hexes[c.from], to: geo.hexes[c.to], troops: c.send },
            None => self.regroup(&geo, me, &mine, view.tick).unwrap_or(Action::NoOp),
        };
        self.geo = Some(geo);
        PlayerCommand::action(action)
    }
}
