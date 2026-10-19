//! Relay: a tempo brain. Every tick should turn troops into land, and land once taken
//! should stay taken. Captures leave both the launch pad and the new hex thick enough
//! that a neighbouring stack can't cheaply take them back, and when no pad can afford a
//! good capture the action relays the nearest spare stack toward the pad that needs it.

use crate::{Bot, PlayerCommand, Style};
use hexarena_protocol::*;
use std::collections::VecDeque;

struct Knobs {
    /// Captures send this % of the expected loss on top of the new hex's garrison.
    margin_pct: u64,
    /// Flat troops added to every attack so tiny fights don't fail on bad dice.
    margin_abs: u64,
    /// A hex next to an enemy keeps this % of what the biggest adjacent enemy stack
    /// could kill if it went all in.
    safety_pct: u64,
    /// Fewest troops left on a hex that touches an enemy.
    guard_floor: u64,
    /// When the pad stays exposed, this % of its surplus rides with the attack.
    forward_pct: u64,
    /// Score of one hex of land.
    land_value: i64,
    /// Extra score for taking a rival's hex (it also costs them land).
    enemy_bonus: i64,
    /// Extra score for taking a hex of the rival we know the most land of.
    leader_bonus: i64,
    /// Score per owned neighbour of the target (compact growth, fewer exposed hexes).
    hug_value: i64,
    /// Score per own hex that stops touching foreign land once the target falls.
    unlock_value: i64,
    /// Score lost per 100 troops of expected loss.
    cost_per_100: i64,
    /// Score per hex of a rival pocket that surrenders when the target falls.
    enclose_value: i64,
    /// Gather instead of relaying single stacks while no enemy touches our land (0/1).
    calm_gather: i64,
    /// Score a blocked capture must beat the best affordable one by before we spend
    /// the tick relaying troops to it instead.
    patience: i64,
    /// Relay search depth (hexes) from a blocked pad.
    relay_depth: i64,
}

impl Knobs {
    fn read(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Knobs {
            margin_pct: k("margin_pct", 150) as u64,
            margin_abs: k("margin_abs", 6) as u64,
            safety_pct: k("safety_pct", 60) as u64,
            guard_floor: k("guard_floor", 30) as u64,
            forward_pct: k("forward_pct", 50) as u64,
            land_value: k("land_value", 100),
            enemy_bonus: k("enemy_bonus", 30),
            leader_bonus: k("leader_bonus", 0),
            hug_value: k("hug_value", 12),
            unlock_value: k("unlock_value", 10),
            cost_per_100: k("cost_per_100", 40),
            enclose_value: k("enclose_value", 100),
            calm_gather: k("calm_gather", 1),
            patience: k("patience", 1000),
            relay_depth: k("relay_depth", 12),
        }
    }
}

#[derive(Clone, Copy)]
struct Cell {
    owner: Option<PlayerId>,
    troops: u32,
    wall: u32,
}

const UNSEEN: Cell = Cell { owner: None, troops: 10, wall: MILLI };

/// A capture we could make from `from`, or are short of troops for.
#[derive(Clone, Copy)]
struct Plan {
    from: usize,
    to: usize,
    score: i64,
    send: u32,
    /// Troops the pad lacks to launch it (0 = affordable now).
    short: u64,
}

pub struct Relay {
    name: String,
    k: Knobs,
    rules: RulesConfig,
    radius: i32,
    hexes: Vec<Hex>,
    nbrs: Vec<[Option<usize>; 6]>,
    terrain: Vec<Option<Terrain>>,
    cells: Vec<Cell>,
    me: PlayerId,
    /// Per own hex this tick: passable neighbours we don't own.
    foreign: Vec<u8>,
    /// Per own hex this tick: troops it should keep against adjacent enemies.
    need: Vec<u64>,
    mark: Vec<u32>,
    stamp: u32,
    /// Rival hexes that surrender if we take this hex (this tick).
    pocket: Vec<i64>,
    pocket_hexes: Vec<usize>,
    leader: Option<PlayerId>,
}

impl Relay {
    pub fn new(style: Style) -> Self {
        Relay {
            name: style.id.clone(),
            k: Knobs::read(&style),
            rules: RulesConfig::default(),
            radius: -1,
            hexes: vec![],
            nbrs: vec![],
            terrain: vec![],
            cells: vec![],
            me: 0,
            foreign: vec![],
            need: vec![],
            mark: vec![],
            stamp: 0,
            pocket: vec![],
            pocket_hexes: vec![],
            leader: None,
        }
    }

    fn index(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * (2 * r + 1) + h.q + r) as usize)
    }

    fn init(&mut self, view: &View) {
        self.radius = view.radius;
        let w = (2 * view.radius + 1) as usize;
        self.hexes = (0..w * w)
            .map(|i| Hex::new((i % w) as i32 - view.radius, (i / w) as i32 - view.radius))
            .collect();
        self.nbrs = self
            .hexes
            .iter()
            .map(|h| h.neighbors().map(|n| self.index(n)))
            .collect();
        self.terrain = view.public_terrain.clone();
        self.cells = vec![UNSEEN; w * w];
        self.foreign = vec![0; w * w];
        self.need = vec![0; w * w];
        self.mark = vec![0; w * w];
        self.pocket = vec![0; w * w];
        self.me = view.you;
    }

    fn passable(&self, i: usize) -> bool {
        self.terrain[i].is_some_and(|t| t.passable())
    }

    fn height(&self, i: usize) -> i8 {
        self.terrain[i].map_or(0, |t| t.height)
    }

    fn own(&self, i: usize) -> bool {
        self.cells[i].owner == Some(self.me)
    }

    fn enemy(&self, i: usize) -> bool {
        self.cells[i].owner.is_some_and(|o| o != self.me)
    }

    fn defense(&self, from: usize, to: usize) -> u64 {
        let h = self.rules.height_multiplier_milli(self.height(from), self.height(to)) as u64;
        (h * self.cells[to].wall as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64)
    }

    /// Expected attacker losses to kill `def` troops on `to` attacking from `from`.
    fn loss(&self, from: usize, to: usize, def: u32) -> u64 {
        let p = self.rules.attacker_exchange_win_milli as u64;
        def as u64 * self.defense(from, to) * (1000 - p) / p / 1000
    }

    /// Defenders a stack of `a` troops on `from` expects to kill on `to` going all in.
    fn kills(&self, from: usize, to: usize, a: u32) -> u64 {
        let p = self.rules.attacker_exchange_win_milli as u64;
        a as u64 * p * 1000 / ((1000 - p) * self.defense(from, to).max(1))
    }

    /// Troops hex `i` should hold once we own it, ignoring `taken` as a threat.
    fn need_at(&self, i: usize, taken: Option<usize>) -> u64 {
        let mut worst = 0;
        let mut touched = false;
        for n in self.nbrs[i].into_iter().flatten() {
            if Some(n) == taken || !self.enemy(n) {
                continue;
            }
            touched = true;
            worst = worst.max(self.kills(n, i, self.cells[n].troops) * self.k.safety_pct / 100);
        }
        if touched {
            worst.max(self.k.guard_floor)
        } else {
            0
        }
    }

    /// Troops own hex `s` must keep after `t` (a neighbour) becomes ours.
    fn keep_after(&self, s: usize, t: usize) -> u64 {
        if self.foreign[s] <= 1 {
            0
        } else if self.enemy(t) {
            self.need_at(s, Some(t))
        } else {
            self.need[s]
        }
    }

    /// Spare troops on own hex `i` beyond what it must keep.
    fn spare(&self, i: usize) -> u64 {
        (self.cells[i].troops as u64).saturating_sub(1 + self.need[i])
    }

    /// Finds rival pockets touching our land that one capture would make surrender, and
    /// credits the hexes whose capture does it in `self.pocket`.
    fn find_pockets(&mut self, own: &[usize]) {
        for i in self.pocket_hexes.drain(..) {
            self.pocket[i] = 0;
        }
        self.stamp += 1;
        let done = self.stamp;
        let mut comp: Vec<usize> = vec![];
        let mut stack: Vec<usize> = vec![];
        for &s in own {
            if self.foreign[s] == 0 {
                continue;
            }
            for seed in self.nbrs[s].into_iter().flatten() {
                let Some(o) = self.cells[seed].owner.filter(|&o| o != self.me) else {
                    continue;
                };
                if self.mark[seed] == done {
                    continue;
                }
                comp.clear();
                stack.clear();
                comp.push(seed);
                stack.push(seed);
                self.mark[seed] = done;
                // The one non-own hex on the pocket's rim, if there's only one.
                let mut gap: Option<usize> = None;
                let mut open = false;
                while let Some(c) = stack.pop() {
                    for m in self.nbrs[c] {
                        let Some(m) = m.filter(|&m| self.passable(m)) else {
                            open = true;
                            continue;
                        };
                        if self.own(m) {
                            continue;
                        }
                        if self.cells[m].owner == Some(o) {
                            if self.mark[m] != done {
                                self.mark[m] = done;
                                comp.push(m);
                                stack.push(m);
                            }
                        } else if gap.is_some_and(|g| g != m) {
                            open = true;
                        } else {
                            gap = Some(m);
                        }
                    }
                    if comp.len() > 48 {
                        open = true;
                    }
                    if open {
                        break;
                    }
                }
                if open {
                    // Mark the rest of a big or open component so it isn't walked again.
                    while let Some(c) = stack.pop() {
                        for m in self.nbrs[c].into_iter().flatten() {
                            if self.mark[m] != done && self.cells[m].owner == Some(o) {
                                self.mark[m] = done;
                                stack.push(m);
                            }
                        }
                    }
                    continue;
                }
                match gap {
                    Some(g) => {
                        self.pocket[g] += comp.len() as i64;
                        self.pocket_hexes.push(g);
                    }
                    None => {
                        for &c in &comp {
                            self.pocket[c] += comp.len() as i64 - 1;
                            self.pocket_hexes.push(c);
                        }
                    }
                }
            }
        }
    }

    fn plan(&self, s: usize, t: usize) -> Plan {
        let c = self.cells[t];
        let loss = self.loss(s, t, c.troops);
        let need_t = self.need_at(t, None);
        let send_min = loss * self.k.margin_pct / 100 + self.k.margin_abs + need_t;
        let keep_s = self.keep_after(s, t);
        let have = (self.cells[s].troops as u64).saturating_sub(1 + keep_s);
        let short = send_min.saturating_sub(have);
        let send = if short > 0 {
            0
        } else {
            let fwd = if keep_s == 0 { 100 } else { self.k.forward_pct.min(100) };
            send_min + (have - send_min) * fwd / 100
        };
        let (mut hug, mut unlock) = (0, 0);
        for n in self.nbrs[t].into_iter().flatten() {
            if self.own(n) {
                hug += 1;
                if self.foreign[n] == 1 {
                    unlock += 1;
                }
            }
        }
        let k = &self.k;
        let enemy = self.enemy(t);
        let mut score = k.land_value + k.hug_value * hug + k.unlock_value * unlock
            - loss as i64 * k.cost_per_100 / 100;
        if enemy {
            score += k.enemy_bonus;
            if c.owner == self.leader {
                score += k.leader_bonus;
            }
        }
        score += k.enclose_value * self.pocket[t];
        Plan { from: s, to: t, score, send: send as u32, short }
    }

    /// One step of the best spare stack toward `pad` through our land.
    fn relay_to(&self, pad: usize, short: u64) -> Option<Action> {
        let mut parent = vec![usize::MAX; self.cells.len()];
        parent[pad] = pad;
        let mut queue = VecDeque::from([(pad, 0u32)]);
        let mut best: Option<(u64, usize)> = None;
        while let Some((h, d)) = queue.pop_front() {
            if h != pad {
                let spare = self.spare(h);
                if spare > 0 {
                    // Enough to launch soonest wins; otherwise the most troops per step.
                    let value = spare.min(short.max(1)) * 1000 / d as u64;
                    if best.map_or(true, |(v, _)| value > v) {
                        best = Some((value, h));
                    }
                }
            }
            if d as i64 >= self.k.relay_depth {
                continue;
            }
            for n in self.nbrs[h].into_iter().flatten() {
                if parent[n] == usize::MAX && self.own(n) {
                    parent[n] = h;
                    queue.push_back((n, d + 1));
                }
            }
        }
        let (_, from) = best?;
        let troops = self.spare(from) as u32;
        Some(Action::Move { from: self.hexes[from], to: self.hexes[parent[from]], troops })
    }

    fn capture(&self, p: Plan) -> Action {
        Action::Move { from: self.hexes[p.from], to: self.hexes[p.to], troops: p.send }
    }

    fn decide(&mut self) -> Action {
        let own: Vec<usize> = (0..self.cells.len()).filter(|&i| self.own(i)).collect();
        let mut land = vec![0u32; 0];
        let mut contact = false;
        for i in 0..self.cells.len() {
            if let Some(o) = self.cells[i].owner.filter(|&o| o != self.me) {
                if land.len() <= o as usize {
                    land.resize(o as usize + 1, 0);
                }
                land[o as usize] += 1;
            }
        }
        self.leader = (0..land.len())
            .filter(|&o| land[o] > 0)
            .max_by_key(|&o| (land[o], std::cmp::Reverse(o)))
            .map(|o| o as PlayerId);
        for &i in &own {
            self.foreign[i] = self.nbrs[i]
                .into_iter()
                .flatten()
                .filter(|&n| self.passable(n) && !self.own(n))
                .count() as u8;
            self.need[i] = if self.foreign[i] > 0 { self.need_at(i, None) } else { 0 };
        }
        self.find_pockets(&own);
        let mut best: Option<Plan> = None;
        let mut blocked: Option<Plan> = None;
        for &s in &own {
            if self.foreign[s] == 0 {
                continue;
            }
            for t in self.nbrs[s].into_iter().flatten() {
                if !self.passable(t) || self.own(t) {
                    continue;
                }
                contact |= self.enemy(t);
                let p = self.plan(s, t);
                if p.short == 0 {
                    if best.map_or(true, |b| p.score > b.score) {
                        best = Some(p);
                    }
                } else if blocked.map_or(true, |b| {
                    (p.score, std::cmp::Reverse(p.short)) > (b.score, std::cmp::Reverse(b.short))
                }) {
                    blocked = Some(p);
                }
            }
        }
        let wait = blocked.filter(|b| best.map_or(true, |p| b.score > p.score + self.k.patience));
        if let (Some(p), None) = (best, wait) {
            return self.capture(p);
        }
        let Some(b) = blocked else {
            return Action::NoOp;
        };
        if self.k.calm_gather > 0 && !contact && own.len() > 2 {
            return Action::Gather { target: self.hexes[b.from] };
        }
        self.relay_to(b.from, b.short)
            .or_else(|| best.map(|p| self.capture(p)))
            .unwrap_or(Action::NoOp)
    }
}

impl Bot for Relay {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if self.radius != view.radius {
            self.init(view);
        }
        self.stamp += 1;
        let seen = self.stamp;
        for v in &view.visible_hexes {
            if let Some(i) = self.index(v.hex) {
                self.cells[i] = Cell { owner: v.owner, troops: v.troops, wall: v.wall_milli };
                self.mark[i] = seen;
            }
        }
        // Our own land is always in sight, so a remembered hex of ours out of sight is lost.
        for i in 0..self.cells.len() {
            if self.mark[i] != seen && self.own(i) {
                self.cells[i] = UNSEEN;
            }
        }
        PlayerCommand::action(self.decide())
    }
}
