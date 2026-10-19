//! Predator: a border-aware conqueror for crowded free-for-alls.
//!
//! Greedy land racers trade the same border hexes back and forth all game, because every
//! capture leaves a thin garrison that is the cheapest thing on the neighbour's menu.
//! Predator instead garrisons what it takes (spare troops ride forward with the front, a
//! reserve stays on sources that still face enemies), so rivals find each other cheaper
//! and churn among themselves while Predator's land sticks. It prefers captures that make
//! enemy pockets surrender, finishes rivals it sees shrinking, and walks stranded interior
//! stacks to the front instead of letting them idle.

use crate::{Bot, Style};
use hexarena_protocol::*;

/// Last known state of a hex.
#[derive(Clone, Copy)]
struct Seen {
    owner: Option<PlayerId>,
    troops: u32,
    wall: u32,
}

struct Knobs {
    /// Score of one more hex, in troop units.
    land: i64,
    /// Extra score when the hex is taken from a rival.
    enemy: i64,
    /// Extra score when the rival is small enough to finish off.
    prey: i64,
    /// A rival with at most this many remembered hexes is prey.
    prey_land: i64,
    /// A rival we can see whole (its every hex and their surroundings are in view) is
    /// also prey when we hold at least this many times (in %) its land.
    prey_ratio_pct: i64,
    /// Score per owned neighbour of the target (compact borders are cheaper to hold).
    hug: i64,
    /// Score per hex that surrenders when the target is taken.
    enclose: i64,
    /// % of the expected troop loss charged against a capture.
    cost_pct: i64,
    /// Against neutral land and prey, troops lose value as they pile up: the loss charge
    /// is scaled by scarce / (scarce + our average troops per hex). Peers pay full price,
    /// so equals settle into quiet borders instead of bleeding each other.
    scarce: i64,
    /// % of every troop missing from a garrison target charged against a capture.
    expose_pct: i64,
    /// Garrison wanted on a hex facing rivals: base + % of the biggest adjacent rival stack.
    hold_base: i64,
    hold_pct: i64,
    hold_cap: i64,
    /// Safety margin on top of the expected loss, in tenths of a standard deviation.
    sigma_tenths: i64,
    /// Interior stacks of at least this many troops walk toward the front when idle.
    feed_min: i64,
    /// Every Nth tick an interior stack walks to the front before any capture (0 = only idle).
    feed_every: i64,
    /// Captures scoring below this are skipped in favour of feeding or waiting.
    min_score: i64,
}

impl Knobs {
    fn from(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d);
        Knobs {
            land: k("land", 60),
            enemy: k("enemy", 20),
            prey: k("prey", 60),
            prey_land: k("prey_land", 25),
            prey_ratio_pct: k("prey_ratio_pct", 300),
            hug: k("hug", 8),
            enclose: k("enclose", 40),
            cost_pct: k("cost_pct", 100),
            scarce: k("scarce", 50),
            expose_pct: k("expose_pct", 60),
            hold_base: k("hold_base", 30),
            hold_pct: k("hold_pct", 50),
            hold_cap: k("hold_cap", 400),
            sigma_tenths: k("sigma_tenths", 25),
            feed_min: k("feed_min", 200),
            feed_every: k("feed_every", 0),
            min_score: k("min_score", 0),
        }
    }
}

pub struct Predator {
    name: String,
    k: Knobs,
    rules: RulesConfig,
    /// Chance (milli) an attacker wins an exchange this tick; rises in overtime.
    win: u32,
    radius: i32,
    seen: Vec<Option<Seen>>,
    own: Vec<bool>,
}

struct Capture {
    score: i64,
    from: Hex,
    to: Hex,
    send: u32,
}

impl Predator {
    pub fn new(style: Style) -> Self {
        Predator {
            name: style.id.clone(),
            k: Knobs::from(&style),
            rules: RulesConfig::default(),
            win: RulesConfig::default().attacker_exchange_win_milli,
            radius: 0,
            seen: vec![],
            own: vec![],
        }
    }

    fn idx(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * (2 * r + 1) + h.q + r) as usize)
    }

    fn terrain(view: &View, i: usize) -> Option<Terrain> {
        view.public_terrain.get(i).copied().flatten()
    }

    fn passable(&self, view: &View, h: Hex) -> bool {
        self.idx(h)
            .and_then(|i| Self::terrain(view, i))
            .is_some_and(|t| t.passable())
    }

    fn height(&self, view: &View, h: Hex) -> i8 {
        self.idx(h)
            .and_then(|i| Self::terrain(view, i))
            .map_or(0, |t| t.height)
    }

    fn mine(&self, h: Hex) -> bool {
        self.idx(h).is_some_and(|i| self.own[i])
    }

    fn rival_at(&self, h: Hex, me: PlayerId) -> Option<(PlayerId, u32)> {
        let s = self.seen[self.idx(h)?]?;
        s.owner.filter(|&o| o != me).map(|o| (o, s.troops))
    }

    fn remember(&mut self, view: &View) {
        if self.seen.is_empty() {
            self.radius = view.radius;
            let w = (2 * view.radius + 1) as usize;
            self.seen = vec![None; w * w];
            self.own = vec![false; w * w];
        }
        for (i, o) in self.own.iter_mut().enumerate() {
            if *o {
                *o = false;
                // Lost out of sight: some rival holds it now.
                if let Some(s) = self.seen[i].as_mut() {
                    s.owner = Some(PlayerId::MAX);
                }
            }
        }
        for c in &view.visible_hexes {
            let Some(i) = self.idx(c.hex) else { continue };
            self.seen[i] = Some(Seen { owner: c.owner, troops: c.troops, wall: c.wall_milli });
            self.own[i] = c.owner == Some(view.you);
        }
    }

    /// Biggest rival stack next to `h`, ignoring `skip` (a hex about to change hands).
    fn threat(&self, h: Hex, skip: Hex, me: PlayerId) -> u32 {
        h.neighbors()
            .into_iter()
            .filter(|&n| n != skip && !self.mine(n))
            .filter_map(|n| self.rival_at(n, me).map(|(_, t)| t))
            .max()
            .unwrap_or(0)
    }

    /// Garrison wanted on our hex `h` once `taken` is ours too.
    fn hold(&self, h: Hex, taken: Hex, me: PlayerId) -> i64 {
        let t = self.threat(h, taken, me);
        if t == 0 {
            return 0;
        }
        (self.k.hold_base + t as i64 * self.k.hold_pct / 100).min(self.k.hold_cap)
    }

    /// (expected attacker loss, standard deviation) of taking `def` troops at `to_h`
    /// behind `wall` from height `from_h`.
    fn fight(&self, from_h: i8, to_h: i8, def: u32, wall: u32) -> (f64, f64) {
        let d = (self.rules.height_multiplier_milli(from_h, to_h) as u64 * wall as u64 / 1000)
            .min(self.rules.maximum_total_defense_milli as u64)
            .max(1);
        let per_win = (1_000_000 + d / 2) / d;
        let wins = (def as u64 * 1000).div_ceil(per_win) as f64;
        let p = self.win as f64 / 1000.0;
        (wins * (1.0 - p) / p, (wins * (1.0 - p)).sqrt() / p)
    }

    /// Hexes that would surrender if `t` became ours: rival components next to `t` that
    /// our land (plus `t`) would fully enclose. Bounded search; unknown land breaks it.
    fn enclosed_by(&self, view: &View, t: Hex, me: PlayerId) -> i64 {
        const CAP: usize = 48;
        let mut total = 0;
        let mut done: Vec<Hex> = vec![];
        for n in t.neighbors() {
            if done.contains(&n) {
                continue;
            }
            let Some((owner, _)) = self.rival_at(n, me) else { continue };
            if self.mine(n) {
                continue;
            }
            let mut comp = vec![n];
            let mut stack = vec![n];
            let mut ok = true;
            'bfs: while let Some(c) = stack.pop() {
                for m in c.neighbors() {
                    if m == t || self.mine(m) || comp.contains(&m) {
                        continue;
                    }
                    if !self.passable(view, m) {
                        ok = false;
                        break 'bfs;
                    }
                    if self.rival_at(m, me).map(|(o, _)| o) != Some(owner) {
                        ok = false;
                        break 'bfs;
                    }
                    comp.push(m);
                    stack.push(m);
                    if comp.len() > CAP {
                        ok = false;
                        break 'bfs;
                    }
                }
            }
            if ok {
                total += comp.len() as i64;
            }
            done.extend(comp);
        }
        total
    }

    fn best_capture(
        &self,
        view: &View,
        mine: &[(Hex, u32)],
        land: &[i64],
        whole: &[Option<i64>],
    ) -> Option<Capture> {
        let me = view.you;
        let k = &self.k;
        let total: u64 = mine.iter().map(|(_, t)| *t as u64).sum();
        let avg = total as f64 / mine.len().max(1) as f64;
        let full_w = k.cost_pct as f64 / 100.0;
        let cheap_w = full_w * k.scarce as f64 / (k.scarce as f64 + avg);
        let ours = mine.len() as i64;
        let prey = |o: PlayerId| {
            land.get(o as usize).is_some_and(|&l| l <= k.prey_land)
                || whole
                    .get(o as usize)
                    .copied()
                    .flatten()
                    .is_some_and(|l| ours * 100 >= l * k.prey_ratio_pct)
        };
        let mut enclose_cache: Vec<(Hex, i64)> = vec![];
        let mut best: Option<Capture> = None;
        for &(c, troops) in mine {
            if troops < 2 {
                continue;
            }
            let ch = self.height(view, c);
            for t in c.neighbors() {
                if self.mine(t) || !self.passable(view, t) {
                    continue;
                }
                let Some(s) = self.idx(t).and_then(|i| self.seen[i]) else { continue };
                let (loss, sd) = self.fight(ch, self.height(view, t), s.troops, s.wall);
                let need = (loss + sd * k.sigma_tenths as f64 / 10.0).ceil() as i64 + 2;
                let spare = troops as i64 - 1;
                if spare < need {
                    continue;
                }
                let keep = self.hold(c, t, me).max(1);
                let send = (troops as i64 - keep).max(need);
                let left = send as f64 - loss;
                let short_t = (self.hold(t, t, me) as f64 - left).max(0.0);
                let short_c = (keep - (troops as i64 - send)).max(0) as f64;
                let hugged = t.neighbors().iter().filter(|n| self.mine(**n)).count() as i64;
                let rival = s.owner.filter(|&o| o != me && o != PlayerId::MAX);
                let is_prey = rival.map_or(false, prey);
                let cost_w = if rival.is_some() && !is_prey { full_w } else { cheap_w };
                let mut score = k.land + k.hug * hugged
                    - (loss * cost_w) as i64
                    - ((short_t + short_c) * k.expose_pct as f64 / 100.0) as i64;
                if rival.is_some() {
                    score += k.enemy;
                    if is_prey {
                        score += k.prey;
                    }
                    let enc = match enclose_cache.iter().find(|(h, _)| *h == t) {
                        Some(&(_, e)) => e,
                        None => {
                            let e = self.enclosed_by(view, t, me);
                            enclose_cache.push((t, e));
                            e
                        }
                    };
                    score += k.enclose * enc;
                }
                if best.as_ref().map_or(true, |b| score > b.score) {
                    best = Some(Capture { score, from: c, to: t, send: send as u32 });
                }
            }
        }
        best
    }

    /// Land of each rival we can currently see entirely: every visible hex of theirs has
    /// its whole neighbourhood in view, so nothing of theirs hides past the fog edge.
    fn whole_rivals(&self, view: &View) -> Vec<Option<i64>> {
        let n = view.public_players.len();
        let w = (2 * self.radius + 1) as usize;
        let mut vis = vec![false; w * w];
        for c in &view.visible_hexes {
            if let Some(i) = self.idx(c.hex) {
                vis[i] = true;
            }
        }
        let mut out: Vec<Option<i64>> = vec![Some(0); n];
        for c in &view.visible_hexes {
            let Some(o) = c.owner.filter(|&o| o != view.you && (o as usize) < n) else {
                continue;
            };
            let edge = c.hex.neighbors().into_iter().any(|m| {
                self.passable(view, m) && !self.idx(m).is_some_and(|i| vis[i])
            });
            out[o as usize] = match out[o as usize] {
                Some(l) if !edge => Some(l + 1),
                _ => None,
            };
        }
        for (p, o) in out.iter_mut().enumerate() {
            if !view.public_players[p].alive || p == view.you as usize || *o == Some(0) {
                *o = None;
            }
        }
        out
    }

    /// The biggest interior stack takes one step toward the nearest hex facing land we
    /// could take, so troops stranded behind the front rejoin the fight.
    fn feed(&self, view: &View, mine: &[(Hex, u32)]) -> Option<Action> {
        let frontier = |h: Hex| {
            h.neighbors()
                .into_iter()
                .any(|n| !self.mine(n) && self.passable(view, n))
        };
        let &(from, troops) = mine
            .iter()
            .filter(|(h, t)| *t as i64 >= self.k.feed_min && !frontier(*h))
            .max_by_key(|(h, t)| (*t, std::cmp::Reverse(*h)))?;
        let w = (2 * self.radius + 1) as usize;
        let mut first = vec![None::<Hex>; w * w];
        let mut queue = std::collections::VecDeque::from([from]);
        first[self.idx(from)?] = Some(from);
        while let Some(h) = queue.pop_front() {
            if h != from && frontier(h) {
                let to = first[self.idx(h)?]?;
                return Some(Action::Move { from, to, troops: troops - 1 });
            }
            for n in h.neighbors() {
                let Some(i) = self.idx(n) else { continue };
                if self.own[i] && first[i].is_none() {
                    first[i] = Some(if h == from { n } else { first[self.idx(h)?]? });
                    queue.push_back(n);
                }
            }
        }
        None
    }

    /// Nothing affordable yet: pull troops toward the border hex facing the cheapest target.
    fn rally(&self, view: &View, mine: &[(Hex, u32)]) -> Option<Action> {
        if mine.len() < 2 {
            return None;
        }
        let mut best: Option<(f64, Hex)> = None;
        for &(c, _) in mine {
            let ch = self.height(view, c);
            for t in c.neighbors() {
                if self.mine(t) || !self.passable(view, t) {
                    continue;
                }
                let Some(s) = self.idx(t).and_then(|i| self.seen[i]) else { continue };
                let (loss, _) = self.fight(ch, self.height(view, t), s.troops, s.wall);
                if best.map_or(true, |(b, _)| loss < b) {
                    best = Some((loss, c));
                }
            }
        }
        best.map(|(_, at)| Action::Gather { target: at })
    }
}

impl Bot for Predator {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        self.win = self.rules.attacker_win_milli(view.tick);
        self.remember(view);
        let me = view.you;
        let mine: Vec<(Hex, u32)> = view
            .visible_hexes
            .iter()
            .filter(|c| c.owner == Some(me))
            .map(|c| (c.hex, c.troops))
            .collect();
        let mut land = vec![0i64; view.public_players.len()];
        for s in self.seen.iter().flatten() {
            if let Some(o) = s.owner.filter(|&o| (o as usize) < land.len()) {
                land[o as usize] += 1;
            }
        }
        let whole = self.whole_rivals(view);
        let fe = self.k.feed_every;
        if fe > 0 && view.tick as i64 % fe == 0 {
            if let Some(a) = self.feed(view, &mine) {
                return PlayerCommand::action(a);
            }
        }
        let action = match self.best_capture(view, &mine, &land, &whole) {
            Some(c) if c.score >= self.k.min_score => {
                Action::Move { from: c.from, to: c.to, troops: c.send }
            }
            other => self
                .feed(view, &mine)
                .or_else(|| other.map(|c| Action::Move { from: c.from, to: c.to, troops: c.send }))
                .or_else(|| self.rally(view, &mine))
                .unwrap_or(Action::NoOp),
        };
        PlayerCommand::action(action)
    }
}
