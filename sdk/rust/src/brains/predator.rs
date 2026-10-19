//! Predator: a border-aware conqueror for crowded free-for-alls.
//!
//! Greedy land racers trade the same border hexes back and forth all game, because every
//! capture leaves a thin garrison that is the cheapest thing on the neighbour's menu.
//! Predator instead garrisons what it takes (spare troops ride forward with the front, a
//! reserve stays on sources that still face enemies), so rivals find each other cheaper
//! and churn among themselves while Predator's land sticks.
//!
//! It reads the table from what it can see: neighbours that have attacked it are
//! hostile and get struck back, peaceful ones are left for later, and a rival it can see
//! whole and dwarfs is prey, hunted down regardless of troop cost, with stranded interior
//! stacks walking to that front. Captures that make enemy pockets surrender come first.

use crate::{Bot, Style};
use hexarena_protocol::*;
use std::collections::VecDeque;

/// Last known state of a hex.
#[derive(Clone, Copy)]
struct Seen {
    owner: Option<PlayerId>,
    troops: u32,
    wall: u32,
}

/// Placeholder owner for a hex we lost out of sight.
const SOMEONE: PlayerId = PlayerId::MAX;

struct Knobs {
    /// Score of one more hex, in troop units.
    land: i64,
    /// Extra score for a hex taken from a hostile rival: one that keeps attacking us.
    hostile: i64,
    /// Score charged for a hex taken from a rival that isn't hostile.
    peace: i64,
    /// A rival is hostile while its grudge (our hexes it attacked, fading with a
    /// half-life of `grudge_halflife` ticks) is at least this.
    hostile_min: i64,
    grudge_halflife: i64,
    /// Extra score for a hex taken from prey.
    prey: i64,
    /// A rival with at most this many remembered hexes is prey.
    prey_land: i64,
    /// A rival we can see whole (every hex of theirs and its surroundings in view) is
    /// prey when we hold at least this many times (in %) its land.
    prey_ratio_pct: i64,
    /// % of the usual troop charge paid when attacking prey.
    prey_cost_pct: i64,
    /// Score per owned neighbour of the target (compact borders are cheaper to hold).
    hug: i64,
    /// Score per hex that surrenders when the target is taken.
    enclose: i64,
    /// % of the expected troop loss charged against a capture.
    cost_pct: i64,
    /// % of every troop missing from a garrison target charged against a capture.
    expose_pct: i64,
    /// Garrison wanted on a hex facing rivals: base + % of the biggest adjacent rival
    /// stack, capped.
    hold_base: i64,
    hold_pct: i64,
    hold_cap: i64,
    /// Safety margin on top of the expected loss, in tenths of a standard deviation.
    sigma_tenths: i64,
    /// Stacks with at least this many troops above their garrison walk to the front
    /// when there is nothing worth taking.
    feed_min: i64,
    /// Whether stacks on hexes facing land we could take also send their surplus on.
    feed_front: bool,
    /// Whether feeding heads for prey when we know of some.
    hunt: bool,
    /// Captures scoring below this are skipped in favour of feeding.
    min_score: i64,
    /// Extra score for hexes of the rival holding the biggest grudge against us.
    focus: i64,
}

impl Knobs {
    fn from(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d);
        Knobs {
            land: k("land", 80),
            hostile: k("hostile", 20),
            peace: k("peace", 0),
            hostile_min: k("hostile_min", 1),
            grudge_halflife: k("grudge_halflife", 100000),
            prey: k("prey", 60),
            prey_land: k("prey_land", 0),
            prey_ratio_pct: k("prey_ratio_pct", 300),
            prey_cost_pct: k("prey_cost_pct", 100),
            hug: k("hug", 8),
            enclose: k("enclose", 40),
            cost_pct: k("cost_pct", 100),
            expose_pct: k("expose_pct", 60),
            hold_base: k("hold_base", 60),
            hold_pct: k("hold_pct", 50),
            hold_cap: k("hold_cap", 400),
            sigma_tenths: k("sigma_tenths", 25),
            feed_min: k("feed_min", 200),
            feed_front: k("feed_front", 1) > 0,
            hunt: k("hunt", 1) > 0,
            min_score: k("min_score", 20),
            focus: k("focus", 0),
        }
    }
}

/// What we make of one opponent this tick.
#[derive(Clone, Copy, Default)]
struct Rival {
    /// Our hexes they captured or attacked as far as we saw, fading over time.
    grudge: f64,
    prey: bool,
    /// The alive rival with the biggest grudge against us.
    nemesis: bool,
}

struct Capture {
    score: i64,
    from: Hex,
    to: Hex,
    send: u32,
}

pub struct Predator {
    name: String,
    k: Knobs,
    rules: RulesConfig,
    radius: i32,
    seen: Vec<Option<Seen>>,
    own: Vec<bool>,
    rivals: Vec<Rival>,
}

impl Predator {
    pub fn new(style: Style) -> Self {
        Predator {
            name: style.id.clone(),
            k: Knobs::from(&style),
            rules: RulesConfig::default(),
            radius: 0,
            seen: vec![],
            own: vec![],
            rivals: vec![],
        }
    }

    fn slots(&self) -> usize {
        let w = (2 * self.radius + 1) as usize;
        w * w
    }

    fn idx(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * (2 * r + 1) + h.q + r) as usize)
    }

    fn terrain(&self, view: &View, h: Hex) -> Option<Terrain> {
        self.idx(h).and_then(|i| view.public_terrain.get(i).copied().flatten())
    }

    fn passable(&self, view: &View, h: Hex) -> bool {
        self.terrain(view, h).is_some_and(|t| t.passable())
    }

    fn height(&self, view: &View, h: Hex) -> i8 {
        self.terrain(view, h).map_or(0, |t| t.height)
    }

    fn mine(&self, h: Hex) -> bool {
        self.idx(h).is_some_and(|i| self.own[i])
    }

    fn seen_at(&self, h: Hex) -> Option<Seen> {
        self.seen[self.idx(h)?]
    }

    /// Owner and troops of a hex some opponent holds.
    fn rival_at(&self, h: Hex, me: PlayerId) -> Option<(PlayerId, u32)> {
        let s = self.seen_at(h)?;
        s.owner.filter(|&o| o != me).map(|o| (o, s.troops))
    }

    fn remember(&mut self, view: &View) {
        if self.seen.is_empty() {
            self.radius = view.radius;
            self.seen = vec![None; self.slots()];
            self.own = vec![false; self.slots()];
            self.rivals = vec![Rival::default(); view.public_players.len()];
        }
        for (i, o) in self.own.iter_mut().enumerate() {
            if std::mem::take(o) {
                if let Some(s) = self.seen[i].as_mut() {
                    s.owner = Some(SOMEONE);
                }
            }
        }
        for c in &view.visible_hexes {
            let Some(i) = self.idx(c.hex) else { continue };
            self.seen[i] = Some(Seen { owner: c.owner, troops: c.troops, wall: c.wall_milli });
            self.own[i] = c.owner == Some(view.you);
        }
    }

    /// Updates grudges from what we saw happen to our land last tick, and decides who is
    /// prey: a rival with little remembered land, or one we can see whole and dwarf.
    fn read_table(&mut self, view: &View, ours: i64) {
        let me = view.you;
        let n = self.rivals.len();
        let fade = 0.5f64.powf(1.0 / self.k.grudge_halflife.max(1) as f64);
        for r in self.rivals.iter_mut() {
            r.grudge *= fade;
        }
        for e in &view.visible_events {
            let (who, hits) = match e {
                Event::Captured { player, previous_owner: Some(p), .. } if *p == me => (*player, 1),
                Event::Surrendered { capturer, from, hexes } if *from == me => {
                    (*capturer, hexes.len() as u32)
                }
                Event::Repelled { attacker, at, .. } if self.mine(*at) => (*attacker, 1),
                _ => continue,
            };
            if let Some(r) = self.rivals.get_mut(who as usize).filter(|_| who != me) {
                r.grudge += hits as f64;
            }
        }
        let mut seen_land = vec![0i64; n];
        for s in self.seen.iter().flatten() {
            if let Some(o) = s.owner.filter(|&o| (o as usize) < n) {
                seen_land[o as usize] += 1;
            }
        }
        let whole = self.whole_land(view);
        let alive = |p: usize| p != me as usize && view.public_players.get(p).is_some_and(|x| x.alive);
        let nemesis = (0..n)
            .filter(|&p| alive(p) && self.rivals[p].grudge >= self.k.hostile_min.max(1) as f64)
            .max_by(|&a, &b| self.rivals[a].grudge.total_cmp(&self.rivals[b].grudge).then(b.cmp(&a)));
        for p in 0..n {
            self.rivals[p].prey = alive(p)
                && (seen_land[p] <= self.k.prey_land
                    || whole[p].is_some_and(|l| ours * 100 >= l * self.k.prey_ratio_pct));
            self.rivals[p].nemesis = nemesis == Some(p);
        }
    }

    /// Land of each rival we can currently see entirely: every visible hex of theirs has
    /// its whole neighbourhood in view, so nothing of theirs hides past the fog edge.
    fn whole_land(&self, view: &View) -> Vec<Option<i64>> {
        let n = self.rivals.len();
        let mut vis = vec![false; self.slots()];
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
        for o in out.iter_mut() {
            if *o == Some(0) {
                *o = None;
            }
        }
        out
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
        let p = self.rules.attacker_exchange_win_milli as f64 / 1000.0;
        (wins * (1.0 - p) / p, (wins * (1.0 - p)).sqrt() / p)
    }

    /// Hexes that would surrender if `t` became ours: rival components next to `t` that
    /// our land (plus `t`) would fully enclose. Bounded search; unknown land breaks it.
    fn enclosed_by(&self, view: &View, t: Hex, me: PlayerId) -> i64 {
        const CAP: usize = 48;
        let mut total = 0;
        let mut done: Vec<Hex> = vec![];
        for n in t.neighbors() {
            if done.contains(&n) || self.mine(n) {
                continue;
            }
            let Some((owner, _)) = self.rival_at(n, me) else { continue };
            let mut comp = vec![n];
            let mut stack = vec![n];
            let mut ok = true;
            'bfs: while let Some(c) = stack.pop() {
                for m in c.neighbors() {
                    if m == t || self.mine(m) || comp.contains(&m) {
                        continue;
                    }
                    if !self.passable(view, m)
                        || self.rival_at(m, me).map(|(o, _)| o) != Some(owner)
                        || comp.len() >= CAP
                    {
                        ok = false;
                        break 'bfs;
                    }
                    comp.push(m);
                    stack.push(m);
                }
            }
            if ok {
                total += comp.len() as i64;
            }
            done.extend(comp);
        }
        total
    }

    fn best_capture(&self, view: &View, mine: &[(Hex, u32)]) -> Option<Capture> {
        let me = view.you;
        let k = &self.k;
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
                let Some(s) = self.seen_at(t) else { continue };
                let (loss, sd) = self.fight(ch, self.height(view, t), s.troops, s.wall);
                let need = (loss + sd * k.sigma_tenths as f64 / 10.0).ceil() as i64 + 2;
                if troops as i64 - 1 < need {
                    continue;
                }
                let keep = self.hold(c, t, me).max(1);
                let send = (troops as i64 - keep).max(need);
                let left = send as f64 - loss;
                let short_t = (self.hold(t, t, me) as f64 - left).max(0.0);
                let short_c = (keep - (troops as i64 - send)).max(0) as f64;
                let hugged = t.neighbors().iter().filter(|n| self.mine(**n)).count() as i64;
                let rival = s
                    .owner
                    .filter(|&o| o != me && o != SOMEONE)
                    .and_then(|o| self.rivals.get(o as usize));
                let mut cost_pct = k.cost_pct;
                let mut score = k.land + k.hug * hugged
                    - ((short_t + short_c) * k.expose_pct as f64 / 100.0) as i64;
                if let Some(r) = rival {
                    score += if r.grudge >= k.hostile_min as f64 { k.hostile } else { -k.peace };
                    if r.nemesis {
                        score += k.focus;
                    }
                    if r.prey {
                        score += k.prey;
                        cost_pct = cost_pct * k.prey_cost_pct / 100;
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
                score -= (loss * cost_pct as f64 / 100.0) as i64;
                if best.as_ref().map_or(true, |b| score > b.score) {
                    best = Some(Capture { score, from: c, to: t, send: send as u32 });
                }
            }
        }
        best
    }

    /// The biggest stack with troops to spare takes one step toward the front: toward
    /// prey when we know where some is, else toward the nearest land we could take.
    fn feed(&self, view: &View, mine: &[(Hex, u32)]) -> Option<Action> {
        let me = view.you;
        let hunting = self.k.hunt
            && (mine.iter().any(|(h, _)| self.borders_prey(*h, me))
                || self.seen.iter().flatten().any(|s| self.is_prey(s.owner)));
        let frontier =
            |h: Hex| h.neighbors().into_iter().any(|n| !self.mine(n) && self.passable(view, n));
        let goal = |h: Hex| if hunting { self.borders_prey(h, me) } else { frontier(h) };
        let (from, spare) = mine
            .iter()
            .filter(|(h, _)| !goal(*h) && (self.k.feed_front || !frontier(*h)))
            .map(|&(h, t)| (h, t as i64 - self.hold(h, h, me).max(1)))
            .filter(|&(_, s)| s >= self.k.feed_min)
            .max_by_key(|&(h, s)| (s, std::cmp::Reverse(h)))?;
        let mut first = vec![None::<Hex>; self.slots()];
        let mut queue = VecDeque::from([from]);
        first[self.idx(from)?] = Some(from);
        while let Some(h) = queue.pop_front() {
            if h != from && goal(h) {
                let to = first[self.idx(h)?]?;
                return Some(Action::Move { from, to, troops: spare as u32 });
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

    fn is_prey(&self, owner: Option<PlayerId>) -> bool {
        owner.and_then(|o| self.rivals.get(o as usize)).is_some_and(|r| r.prey)
    }

    fn borders_prey(&self, h: Hex, me: PlayerId) -> bool {
        h.neighbors()
            .into_iter()
            .any(|n| self.rival_at(n, me).is_some_and(|(o, _)| self.is_prey(Some(o))))
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
                let Some(s) = self.seen_at(t) else { continue };
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
        self.remember(view);
        let me = view.you;
        let mine: Vec<(Hex, u32)> = view
            .visible_hexes
            .iter()
            .filter(|c| c.owner == Some(me))
            .map(|c| (c.hex, c.troops))
            .collect();
        self.read_table(view, mine.len() as i64);
        let action = match self.best_capture(view, &mine) {
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
