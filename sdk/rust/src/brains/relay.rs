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
    /// Score per 100 enemy troops destroyed by a capture (a stack gone is a threat gone).
    kill_pct: i64,
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
    /// Weight (%) of a blocked capture's score per tick of relaying it needs, against
    /// a capture we can make right now.
    patience_pct: i64,
    /// Extra weight (%) for the blocked capture we started stocking last tick.
    commit_pct: i64,
    /// Blocked captures we cost out each tick, best scores first.
    relay_plans: i64,
    /// Relay search depth (hexes) from a blocked pad.
    relay_depth: i64,
    /// Score of restocking an enemy-facing hex that holds less than it should.
    defend_value: i64,
    /// Gather toward a blocked pad, even in contact, when it would leave fewer than this
    /// many enemy-facing hexes under their garrison (0 = only while no enemy touches us).
    gather_risk: i64,
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
            kill_pct: k("kill_pct", 0),
            leader_bonus: k("leader_bonus", 0),
            hug_value: k("hug_value", 12),
            unlock_value: k("unlock_value", 10),
            cost_per_100: k("cost_per_100", 40),
            enclose_value: k("enclose_value", 100),
            calm_gather: k("calm_gather", 1),
            patience_pct: k("patience_pct", 100),
            commit_pct: k("commit_pct", 50),
            relay_plans: k("relay_plans", 6),
            relay_depth: k("relay_depth", 12),
            gather_risk: k("gather_risk", 3),
            defend_value: k("defend_value", 80),
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

struct Route {
    /// Ticks until the capture lands when relaying single stacks, if troops in reach suffice.
    moves: Option<u32>,
    /// The same when gathering.
    gathers: Option<u32>,
    step: Action,
}

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

/// A blocked plan with its value per tick when stocked by moves or by one Gather.
struct RelayOption {
    plan: Plan,
    step: Action,
    moves: Option<i64>,
    gather: Option<i64>,
    /// Whether the Gather is known not to strip a guarded hex.
    checked: bool,
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
    /// Per own hex (and target) this tick: troops it should keep against adjacent enemies.
    need: Vec<u64>,
    mark: Vec<u32>,
    stamp: u32,
    /// Rival hexes that surrender if we take this hex (this tick).
    pocket: Vec<i64>,
    pocket_hexes: Vec<usize>,
    parent: Vec<usize>,
    dist: Vec<u32>,
    inflow: Vec<u64>,
    /// Bumped every tick; per-hex caches below are valid when their tick matches.
    tick_id: u32,
    need_tick: Vec<u32>,
    exposure: Vec<u32>,
    exposure_tick: Vec<u32>,
    own_prev: Vec<usize>,
    /// The (pad, target) we relayed troops toward last tick.
    commit: Option<(usize, usize)>,
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
            parent: vec![],
            dist: vec![],
            inflow: vec![],
            commit: None,
            tick_id: 0,
            need_tick: vec![],
            exposure: vec![],
            exposure_tick: vec![],
            own_prev: vec![],
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
        self.parent = vec![0; w * w];
        self.dist = vec![0; w * w];
        self.inflow = vec![0; w * w];
        self.need_tick = vec![0; w * w];
        self.exposure = vec![0; w * w];
        self.exposure_tick = vec![0; w * w];
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
        // Each walk gets its own stamp; meeting an earlier walk's hex means joining a
        // component that already proved too big or open.
        let first = self.stamp + 1;
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
                if self.mark[seed] >= first {
                    continue;
                }
                self.stamp += 1;
                let walk = self.stamp;
                comp.clear();
                stack.clear();
                comp.push(seed);
                stack.push(seed);
                self.mark[seed] = walk;
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
                            if self.mark[m] == walk {
                                continue;
                            }
                            if self.mark[m] >= first {
                                open = true;
                                break;
                            }
                            self.mark[m] = walk;
                            comp.push(m);
                            stack.push(m);
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

    fn plan(&mut self, s: usize, t: usize) -> Plan {
        let c = self.cells[t];
        let loss = self.loss(s, t, c.troops);
        if self.need_tick[t] != self.tick_id {
            self.need_tick[t] = self.tick_id;
            self.need[t] = self.need_at(t, None);
        }
        let need_t = self.need[t];
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
            score += k.enemy_bonus + c.troops as i64 * k.kill_pct / 100;
            if c.owner == self.leader {
                score += k.leader_bonus;
            }
        }
        score += k.enclose_value * self.pocket[t];
        Plan { from: s, to: t, score, send: send as u32, short }
    }

    /// How to stock `pad` with `short` more troops: walks our land outward from the pad,
    /// summing spare troops nearest first, until they cover the shortfall at distance
    /// `d` using `n` donors. Relaying single stacks then takes about `d + n` ticks and
    /// gathering `d`, plus the capture. The next relay move is the donor with the most
    /// troops per step, one hex closer.
    fn route(&mut self, pad: usize, short: u64) -> Option<Route> {
        self.stamp += 1;
        let seen = self.stamp;
        self.mark[pad] = seen;
        self.parent[pad] = pad;
        let mut queue = VecDeque::from([(pad, 0u32)]);
        let (mut sum, mut donors) = (0u64, 0u32);
        let mut best: Option<(u64, usize)> = None;
        let mut reach = None;
        while let Some((h, d)) = queue.pop_front() {
            if h != pad {
                let spare = self.spare(h);
                if spare > 0 {
                    sum += spare;
                    donors += 1;
                    let rate = spare.min(short) * 1000 / d as u64;
                    if best.map_or(true, |(r, _)| rate > r) {
                        best = Some((rate, h));
                    }
                    if sum >= short {
                        reach = Some((d, donors));
                        break;
                    }
                }
            }
            if d as i64 >= self.k.relay_depth {
                continue;
            }
            for n in self.nbrs[h].into_iter().flatten() {
                if self.mark[n] != seen && self.own(n) {
                    self.mark[n] = seen;
                    self.parent[n] = h;
                    queue.push_back((n, d + 1));
                }
            }
        }
        let (_, from) = best?;
        let troops = self.spare(from) as u32;
        let step = Action::Move { from: self.hexes[from], to: self.hexes[self.parent[from]], troops };
        Some(Route {
            moves: reach.map(|(d, n)| d + n + 1),
            gathers: reach.map(|(d, _)| d + 1),
            step,
        })
    }

    /// Own hexes touching an enemy that a Gather toward `target` would leave below the
    /// troops they should keep. Mirrors the engine: every hex with spare troops sends
    /// all but one a step down the distance map, to its first neighbour in direction order.
    fn gather_exposure(&mut self, target: usize) -> u32 {
        self.stamp += 1;
        let seen = self.stamp;
        self.mark[target] = seen;
        self.dist[target] = 0;
        let mut order = vec![target];
        let mut head = 0;
        while head < order.len() {
            let h = order[head];
            head += 1;
            for n in self.nbrs[h].into_iter().flatten() {
                if self.mark[n] != seen && self.own(n) {
                    self.mark[n] = seen;
                    self.dist[n] = self.dist[h] + 1;
                    order.push(n);
                }
            }
        }
        for &h in &order {
            self.inflow[h] = 0;
        }
        for &h in &order[1..] {
            let t = self.cells[h].troops;
            if t <= 1 {
                continue;
            }
            let d = self.dist[h];
            if let Some(next) = self.nbrs[h]
                .into_iter()
                .flatten()
                .find(|&n| self.mark[n] == seen && self.dist[n] + 1 == d)
            {
                self.inflow[next] += t as u64 - 1;
            }
        }
        order[1..]
            .iter()
            .filter(|&&h| {
                self.need[h] > 0 && self.cells[h].troops.min(1) as u64 + self.inflow[h] < self.need[h]
            })
            .count() as u32
    }

    fn exposure(&mut self, pad: usize) -> u32 {
        if self.exposure_tick[pad] != self.tick_id {
            self.exposure_tick[pad] = self.tick_id;
            self.exposure[pad] = self.gather_exposure(pad);
        }
        self.exposure[pad]
    }

    fn capture(&self, p: Plan) -> Action {
        Action::Move { from: self.hexes[p.from], to: self.hexes[p.to], troops: p.send }
    }

    fn decide(&mut self, own: &[usize]) -> Action {
        let mut contact = false;
        if self.k.leader_bonus > 0 {
            let mut land = vec![0u32; 0];
            for c in &self.cells {
                if let Some(o) = c.owner.filter(|&o| o != self.me) {
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
        }
        for &i in own {
            self.foreign[i] = self.nbrs[i]
                .into_iter()
                .flatten()
                .filter(|&n| self.passable(n) && !self.own(n))
                .count() as u8;
            self.need[i] = if self.foreign[i] > 0 { self.need_at(i, None) } else { 0 };
            self.need_tick[i] = self.tick_id;
        }
        self.find_pockets(own);
        let mut best: Option<Plan> = None;
        let mut blocked: Vec<Plan> = vec![];
        for &s in own {
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
                } else {
                    blocked.push(p);
                }
            }
        }
        // Hexes under their garrison are stocked like pads: losing one costs a hex and
        // hands the neighbour a launch pad inside our land.
        if self.k.defend_value > 0 {
            for &h in own {
                let have = (self.cells[h].troops as u64).saturating_sub(1);
                if self.need[h] > have {
                    let short = self.need[h] - have;
                    blocked.push(Plan { from: h, to: h, score: self.k.defend_value, send: 0, short });
                }
            }
        }
        // Score per tick: a capture now takes one tick, a blocked one first needs its
        // pad stocked. Only blocked plans that beat the best capture now can win.
        let floor = best.map_or(i64::MIN, |b| b.score);
        blocked.retain(|b| b.score > floor);
        blocked.sort_by_key(|b| (std::cmp::Reverse(b.score), b.short, b.from, b.to));
        blocked.truncate(self.k.relay_plans.max(1) as usize);
        let capture_value = best.map(|b| b.score * 100);
        let mut confirmed = capture_value;
        let mut options: Vec<RelayOption> = vec![];
        let commit = self.commit.take();
        let calm = !contact && own.len() > 2;
        let calm_gather = calm && self.k.calm_gather > 0;
        let (patience, commit_pct) = (self.k.patience_pct, self.k.commit_pct);
        for b in blocked {
            // Every relay takes at least two ticks and plans come best score first, so
            // once one can't beat a sure choice none of the rest can.
            let ceiling = if b.score > 0 { b.score * patience / 2 } else { b.score * 200 };
            if confirmed.is_some_and(|v| ceiling + ceiling.abs() * commit_pct / 100 <= v) {
                break;
            }
            let Some(r) = self.route(b.from, b.short) else {
                continue;
            };
            let committed = commit == Some((b.from, b.to));
            let value = |ticks: u32| {
                let ticks = ticks.max(1) as i64;
                let v = if b.score > 0 { b.score * patience / ticks } else { b.score * 100 * ticks };
                if committed { v + v.abs() * commit_pct / 100 } else { v }
            };
            let option = RelayOption {
                plan: b,
                step: r.step,
                moves: r.moves.map(value),
                gather: r.gathers.filter(|_| calm_gather || self.k.gather_risk > 0).map(value),
                checked: calm_gather,
            };
            let sure = if option.checked { option.gather.max(option.moves) } else { option.moves };
            if sure.is_some_and(|v| confirmed.map_or(true, |c| v > c)) {
                confirmed = sure;
            }
            options.push(option);
        }
        // Simulating a Gather is the costly part, so only the option about to win is
        // checked; a risky one falls back to relaying by moves.
        let relay = loop {
            let mut pick: Option<(usize, i64)> = None;
            for (i, o) in options.iter().enumerate() {
                if let Some(v) = o.gather.or(o.moves) {
                    if capture_value.map_or(true, |c| v > c) && pick.map_or(true, |p| v > p.1) {
                        pick = Some((i, v));
                    }
                }
            }
            let Some((i, _)) = pick else {
                break None;
            };
            let o = &options[i];
            if o.gather.is_some() && !o.checked {
                if (self.exposure(o.plan.from) as i64) < self.k.gather_risk {
                    options[i].checked = true;
                } else {
                    options[i].gather = None;
                    continue;
                }
            }
            let o = &options[i];
            let step = if o.gather.is_some() { Action::Gather { target: self.hexes[o.plan.from] } } else { o.step.clone() };
            break Some((o.plan, step));
        };
        let relay = match (relay, best) {
            (Some(r), _) => r,
            (None, Some(b)) => return self.capture(b),
            (None, None) => {
                // Nothing can be stocked in time: start feeding the pad nearest ready.
                let mut fallback: Option<&RelayOption> = None;
                for o in &options {
                    if fallback.map_or(true, |f| o.plan.short < f.plan.short) {
                        fallback = Some(o);
                    }
                }
                match fallback {
                    Some(o) => (o.plan, o.step.clone()),
                    None => return Action::NoOp,
                }
            }
        };
        let (b, step) = relay;
        self.commit = Some((b.from, b.to));
        if calm && self.k.calm_gather > 0 {
            return Action::Gather { target: self.hexes[b.from] };
        }
        step
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
        self.tick_id += 1;
        let mut own = vec![];
        for v in &view.visible_hexes {
            if let Some(i) = self.index(v.hex) {
                self.cells[i] = Cell { owner: v.owner, troops: v.troops, wall: v.wall_milli };
                if v.owner == Some(self.me) {
                    own.push(i);
                }
            }
        }
        // Our own land is always in sight, so a remembered hex of ours out of sight is lost.
        for &i in &self.own_prev {
            if self.own(i) && own.binary_search(&i).is_err() {
                self.cells[i] = UNSEEN;
            }
        }
        let action = self.decide(&own);
        self.own_prev = own;
        PlayerCommand::action(action)
    }
}
