//! Ridgeline: a border-holding land racer.
//!
//! Late in a free-for-all every seat is limited by its one action a tick, not by troops,
//! so land is decided by how many captured hexes stay captured. Ridgeline sizes every
//! border hex's garrison against the visible enemy stacks next to it, scaled by the
//! height they would attack uphill or downhill from, so high ground and hexes shielded
//! by mountains, water and the map edge are cheap to hold. Captures split the attacking
//! hex's troops so both it and the new hex stay out of reach, are only sent when they
//! can't plausibly bounce, and prefer ground that shortens the exposed border or makes an
//! enemy pocket surrender. Spare actions relay interior stacks toward short border hexes.

use crate::{Action, Bot, Hex, PlayerCommand, PlayerId, RulesConfig, Style, View, MILLI};
use std::collections::VecDeque;

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
            hexes[i] = Hex::new(i as i32 % width - radius, i as i32 / width - radius);
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

/// Tunables, read once from the genome's knobs.
struct Knobs {
    /// Standard deviations (x10) of bad dice every capture must survive.
    sure_x10: u64,
    land: i64,
    enemy: i64,
    hug: i64,
    open: i64,
    height: i64,
    cost_pct: i64,
    /// Troops per hex at which a troop's cost halves (0 = troops always cost full price).
    wealth: u64,
    enclose: i64,
    /// Garrison any enemy-facing hex wants even when no big stack is next to it.
    guard: u64,
    /// % of what the biggest adjacent enemy stack kills going all in that a border hex
    /// wants to hold.
    margin_pct: u64,
    /// Weight of the source hex's lost safety against the new hex's land.
    drop_w: i64,
    supply_every: u32,
    /// Safety gain (milli) a supply move must buy on its cadence tick.
    supply_min: i64,
    /// Captures scoring below `bar - 500` give the tick to supply or regrouping.
    min_score: i64,
    /// Gathers are allowed before this tick while no enemy touches our land.
    gather_until: u32,
}

impl Knobs {
    fn read(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Knobs {
            sure_x10: k("sure_x10", 30) as u64,
            land: k("land", 100),
            enemy: k("enemy", 30),
            hug: k("hug", 25),
            open: k("open", 15),
            height: k("height", 10),
            cost_pct: k("cost_pct", 100),
            wealth: k("wealth", 0) as u64,
            enclose: k("enclose", 100),
            guard: k("guard", 20) as u64,
            margin_pct: k("margin_pct", 100) as u64,
            drop_w: k("drop_w", 100),
            supply_every: k("supply_every", 3) as u32,
            supply_min: k("supply_min", 200),
            min_score: k("bar", 0) - 500,
            gather_until: k("gather_until", 600) as u32,
        }
    }
}

pub struct Ridgeline {
    name: String,
    rules: RulesConfig,
    k: Knobs,
    geo: Option<Geometry>,
    /// Last seen state of every slot.
    seen: Vec<Option<Cell>>,
}

impl Ridgeline {
    pub fn new(style: Style) -> Self {
        Ridgeline {
            name: style.id.clone(),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            geo: None,
            seen: vec![],
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

    /// Defenders `attackers` troops expect to kill behind `defense` milli going all in.
    fn kills(&self, attackers: u64, defense: u64) -> u64 {
        let p = self.rules.attacker_exchange_win_milli as u64;
        attackers * p * 1000 / ((1000 - p) * defense.max(1))
    }

    fn defense(&self, geo: &Geometry, from: usize, to: usize, wall_milli: u32) -> u64 {
        let h = self.rules.height_multiplier_milli(geo.height[from], geo.height[to]) as u64;
        (h * wall_milli as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64)
    }

    fn owned(&self, i: usize, me: PlayerId) -> bool {
        self.seen[i].is_some_and(|c| c.owner == Some(me))
    }

    fn enemy(&self, i: usize, me: PlayerId) -> bool {
        self.seen[i].is_some_and(|c| c.owner.is_some_and(|o| o != me))
    }

    /// Garrison own (or soon own) hex `h` wants against its enemy neighbours, treating
    /// `ours` as ours too; `None` when no enemy touches it.
    fn want(&self, geo: &Geometry, me: PlayerId, h: usize, ours: usize) -> Option<u64> {
        let mut worst: Option<u64> = None;
        for &n in &geo.nbrs[h] {
            if n == ours || !self.enemy(n, me) {
                continue;
            }
            let stack = self.seen[n].unwrap().troops.saturating_sub(1) as u64;
            let k = self.kills(stack, self.defense(geo, n, h, MILLI));
            worst = Some(worst.map_or(k, |w| w.max(k)));
        }
        worst.map(|k| self.k.guard + k * self.k.margin_pct / 100)
    }

    /// How safe (milli) a hex holding `troops` is against `want`: concave, full at `want`.
    fn safety(troops: u64, want: Option<u64>) -> i64 {
        match want {
            None => 1000,
            Some(w) => {
                let r = (troops * 1000 / w.max(1)).min(1000) as i64;
                1000 - (1000 - r) * (1000 - r) / 1000
            }
        }
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

    fn best_capture(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<Capture> {
        let mut best: Option<Capture> = None;
        // A troop is worth less the more of them we already hold per hex.
        let bank: u64 = mine.iter().map(|&i| self.seen[i].unwrap().troops as u64).sum();
        let per_hex = bank / mine.len().max(1) as u64;
        let full = self.k.cost_pct as u64 * 10;
        let cost_milli = match self.k.wealth {
            0 => full,
            w => full * w / (w + per_hex),
        } as i64;
        for &s in mine {
            let src = self.seen[s].unwrap();
            if src.troops < 2 {
                continue;
            }
            let troops = src.troops as u64;
            let avail = troops - 1;
            let want_s_before = self.want(geo, me, s, usize::MAX);
            let before = Self::safety(troops, want_s_before);
            for &t in &geo.nbrs[s] {
                let Some(tc) = self.seen[t] else { continue };
                if tc.owner == Some(me) {
                    continue;
                }
                let defense = self.defense(geo, s, t, tc.wall_milli) as u32;
                let (mean, sd) = self.loss_stats(tc.troops, defense);
                let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
                if need > avail {
                    continue;
                }
                let loss = mean.div_ceil(1000);
                let want_t = self.want(geo, me, t, s);
                let want_s = self.want(geo, me, s, t);
                let mut cands = [0u64; 12];
                for (f, c) in cands.iter_mut().enumerate().take(9) {
                    *c = need + (avail - need) * f as u64 / 8;
                }
                cands[9] = want_t.map_or(avail, |w| loss + w);
                cands[10] = want_s.map_or(avail, |w| troops.saturating_sub(w));
                cands[11] = avail;
                let mut pick = (i64::MIN, 0i64, avail);
                for c in cands {
                    let send = c.clamp(need, avail);
                    let st = Self::safety(send.saturating_sub(loss), want_t);
                    let drop = before - Self::safety(troops - send, want_s);
                    let v = st * 1000 - drop * self.k.drop_w * 10;
                    if (v, send) > (pick.0, pick.2) {
                        pick = (v, st, send);
                    }
                }
                let (v, st, send) = pick;
                let hugged = geo.nbrs[t].iter().filter(|&&n| self.owned(n, me)).count() as i64;
                let exposed = geo.nbrs[t].len() as i64 - hugged;
                let mut gain = self.k.land;
                if tc.owner.is_some() {
                    gain += self.k.enemy + self.k.enclose * self.enclosure_gain(geo, t, me);
                }
                gain += self.k.hug * hugged - self.k.open * exposed
                    + self.k.height * geo.height[t] as i64;
                let score = gain * st / 1000 + (v / 1000 - st) * self.k.land / 1000
                    - loss as i64 * cost_milli / 1000;
                let better = best.as_ref().map_or(true, |b| {
                    (score, std::cmp::Reverse(t), send)
                        > (b.score, std::cmp::Reverse(b.to), b.send as u64)
                });
                if better {
                    best = Some(Capture { score, from: s, to: t, send: send as u32 });
                }
            }
        }
        best
    }

    /// Spare troops on own hex `i` beyond what it wants to keep.
    fn spare(&self, geo: &Geometry, me: PlayerId, i: usize) -> u64 {
        let t = self.seen[i].unwrap().troops as u64;
        let keep = self.want(geo, me, i, usize::MAX).unwrap_or(0).max(1);
        t.saturating_sub(keep)
    }

    /// Relays the spare stack that buys the most border safety per step one hex toward
    /// the short border hex nearest to it. Returns the safety gain per step (milli).
    fn supply(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<(i64, Action)> {
        let slots = geo.passable.len();
        // Multi-source BFS from every short border hex through our land.
        let mut dist = vec![u32::MAX; slots];
        let mut origin = vec![usize::MAX; slots];
        let mut toward = vec![usize::MAX; slots];
        let mut short: Vec<(u64, u64)> = vec![(0, 0); slots];
        let mut queue = VecDeque::new();
        for &f in mine {
            let Some(w) = self.want(geo, me, f, usize::MAX) else { continue };
            let have = self.seen[f].unwrap().troops as u64;
            if have >= w {
                continue;
            }
            short[f] = (have, w);
            dist[f] = 0;
            origin[f] = f;
            queue.push_back(f);
        }
        while let Some(c) = queue.pop_front() {
            for &n in &geo.nbrs[c] {
                if dist[n] == u32::MAX && self.owned(n, me) {
                    dist[n] = dist[c] + 1;
                    origin[n] = origin[c];
                    toward[n] = c;
                    queue.push_back(n);
                }
            }
        }
        let mut best: Option<(i64, usize)> = None;
        for &i in mine {
            if dist[i] == 0 || dist[i] == u32::MAX {
                continue;
            }
            let spare = self.spare(geo, me, i);
            if spare == 0 {
                continue;
            }
            let (have, w) = short[origin[i]];
            let gain = (Self::safety(have + spare, Some(w)) - Self::safety(have, Some(w)))
                / dist[i] as i64;
            if best.map_or(true, |b| (gain, std::cmp::Reverse(i)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((gain, i));
            }
        }
        let (gain, i) = best?;
        let troops = self.spare(geo, me, i) as u32;
        Some((gain, Action::Move { from: geo.hexes[i], to: geo.hexes[toward[i]], troops }))
    }

    /// Nothing worth taking: pull troops toward the border hex facing the cheapest target.
    fn regroup(&self, geo: &Geometry, me: PlayerId, mine: &[usize], tick: u32) -> Option<Action> {
        if mine.len() < 2 {
            return None;
        }
        let mut cheapest: Option<(u32, usize)> = None;
        let mut contact = false;
        for &s in mine {
            for &t in &geo.nbrs[s] {
                let Some(tc) = self.seen[t].filter(|c| c.owner != Some(me)) else { continue };
                contact |= tc.owner.is_some();
                let key = (tc.troops, s);
                if cheapest.map_or(true, |c| key < c) {
                    cheapest = Some(key);
                }
            }
        }
        let (_, pad) = cheapest?;
        if !contact && tick < self.k.gather_until {
            return Some(Action::Gather { target: geo.hexes[pad] });
        }
        // A Gather would strip every far border hex down to one troop, so instead walk
        // the nearest spare stack one step toward the pad.
        let mut prev = vec![usize::MAX; geo.passable.len()];
        prev[pad] = pad;
        let mut queue = VecDeque::from([pad]);
        while let Some(c) = queue.pop_front() {
            if c != pad && self.spare(geo, me, c) >= 2 {
                let troops = self.spare(geo, me, c) as u32;
                return Some(Action::Move { from: geo.hexes[c], to: geo.hexes[prev[c]], troops });
            }
            for &n in &geo.nbrs[c] {
                if prev[n] == usize::MAX && self.owned(n, me) {
                    prev[n] = c;
                    queue.push_back(n);
                }
            }
        }
        None
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
        let supply = self.supply(&geo, me, &mine);
        let capture = self.best_capture(&geo, me, &mine).filter(|c| c.score >= self.k.min_score);
        let supply_tick = self.k.supply_every > 0 && view.tick % self.k.supply_every == 0;
        let action = match (supply, capture) {
            (Some((gain, a)), _) if supply_tick && gain >= self.k.supply_min => a,
            (_, Some(c)) => {
                Action::Move { from: geo.hexes[c.from], to: geo.hexes[c.to], troops: c.send }
            }
            (Some((gain, a)), None) if gain > 0 => a,
            _ => self.regroup(&geo, me, &mine, view.tick).unwrap_or(Action::NoOp),
        };
        self.geo = Some(geo);
        PlayerCommand::action(action)
    }
}
