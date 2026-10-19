//! Warren: a border holder that digs in.
//!
//! The tactical core is a Ridgeline-style land racer: garrisons sized against adjacent
//! enemy stacks, captures that are only sent when sure and when the new hex can be held,
//! spare actions relaying interior troops to the border. On top of it Warren builds:
//! whenever the core has nothing better than a relay move to do, the tick goes to Farms
//! on low interior ground, Walls on the hottest enemy-facing hexes (linked into lines),
//! Villages next to Farms, and Quarries on interior hills. Garrisons count their own
//! Wall, so a walled border holds with a fraction of the troops.

use crate::{
    Action, Bot, Building, Hex, ManagementCommand, PlayerCommand, PlayerId, RulesConfig, Style,
    View, VisibleHex, MILLI,
};
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

/// How the land we don't own hangs together this tick.
struct Seal {
    /// Hexes of small, enemy-free regions that taking this hex would cut off from the
    /// rest of the map behind our land, mountains, water and the map edge.
    cuts: Vec<u32>,
    /// Hexes already inside such a region: nobody else can reach them without first
    /// taking our land, so they can wait.
    pocket: Vec<bool>,
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
    /// Captures leaving the new hex less safe (milli) than this are skipped.
    safe_min: i64,
    supply_every: u32,
    /// Score per hex of backyard a capture seals off.
    seal: i64,
    /// Largest region (hexes) that counts as a backyard.
    seal_max: u32,
    /// Score lost for taking a hex that is already in our backyard.
    backyard: i64,
    /// Over the last this many ticks, race for land that only has to last until the cutoff.
    sprint: u32,
    /// Safety gain (milli) a supply move must buy on its cadence tick.
    supply_min: i64,
    /// Captures scoring below `bar - 500` give the tick to supply or regrouping.
    min_score: i64,
    /// Gathers are allowed before this tick while no enemy touches our land.
    gather_until: u32,
    /// Score a capture we can't afford yet must beat the best affordable one by before
    /// the tick stages troops onto its launch hex instead (0 = never stage).
    stage: i64,
    /// Score lost per troop the launch hex is short of a blocked capture.
    short_cost: i64,
}

impl Knobs {
    fn read(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Knobs {
            sure_x10: k("sure_x10", 30) as u64,
            land: k("land", 100),
            enemy: k("enemy", 30),
            hug: k("hug", 40),
            open: k("open", 15),
            height: k("height", 10),
            cost_pct: k("cost_pct", 100),
            wealth: k("wealth", 30) as u64,
            enclose: k("enclose", 100),
            guard: k("guard", 20) as u64,
            margin_pct: k("margin_pct", 600) as u64,
            drop_w: k("drop_w", 100),
            safe_min: k("safe_min", 300),
            supply_every: k("supply_every", 3) as u32,
            seal: k("seal", 0),
            seal_max: k("seal_max", 40) as u32,
            backyard: k("backyard", 0),
            sprint: k("sprint", 0) as u32,
            supply_min: k("supply_min", 200),
            min_score: k("bar", 500) - 500,
            gather_until: k("gather_until", 600) as u32,
            stage: k("stage", 0),
            short_cost: k("short_cost", 0),
        }
    }
}

/// Builder tunables.
struct BuildKnobs {
    /// Garrisons count the hex's own Wall (0 = ignore it, like Ridgeline).
    wall_aware: bool,
    farms: usize,
    farm_tick: u32,
    /// Walls go on enemy-facing hexes whose biggest adjacent enemy stack is at least this.
    wall_heat: u32,
    /// A hex must hold this many troops beyond the Wall's cost before walling it.
    wall_keep: u32,
    walls: usize,
    villages: usize,
    quarries: usize,
    quarry_tick: u32,
    /// Builds may take a capture's tick when the capture scores below this.
    preempt_below: i64,
    /// No Walls before this tick: early on troops and actions go to land.
    wall_tick: u32,
    /// From this tick plain Villages go on any interior hex (0 = never).
    carpet_tick: u32,
    /// Resource the carpet leaves for Walls.
    carpet_reserve: u32,
    /// Late minimum garrison of enemy-facing hexes (troops behind no Wall; 0 = off),
    /// ramped in over `guard_ramp` ticks from `guard_from`.
    guard_late: u64,
    guard_from: u32,
    guard_ramp: u32,
    /// Hexes this close (ticks) to the storm hold no garrison and are evacuated.
    flee_lead: u32,
    /// Smallest doomed stack worth an evacuation move.
    flee_min: u32,
    /// Captures and builds need at least this many safe ticks left on the hex.
    cap_life: u32,
    village_life: u32,
    /// Plain Villages may also go on enemy-facing hexes.
    carpet_border: bool,
    /// Capture score per 1000 safe ticks a target has left (0 = off).
    centre: i64,
}

impl BuildKnobs {
    fn read(style: &Style) -> BuildKnobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        BuildKnobs {
            wall_aware: k("wall_aware", 1) > 0,
            farms: k("farms", 3) as usize,
            farm_tick: k("farm_tick", 60) as u32,
            wall_heat: k("wall_heat", 20) as u32,
            wall_keep: k("wall_keep", 30) as u32,
            walls: k("walls", 200) as usize,
            villages: k("villages", 9) as usize,
            quarries: k("quarries", 2) as usize,
            quarry_tick: k("quarry_tick", 400) as u32,
            preempt_below: k("preempt_below", 0),
            wall_tick: k("wall_tick", 0) as u32,
            carpet_tick: k("carpet_tick", 0) as u32,
            carpet_reserve: k("carpet_reserve", 10) as u32,
            guard_late: k("guard_late", 0) as u64,
            guard_from: k("guard_from", 800) as u32,
            guard_ramp: k("guard_ramp", 600) as u32,
            flee_lead: k("flee_lead", 0) as u32,
            flee_min: k("flee_min", 20) as u32,
            cap_life: k("cap_life", 0) as u32,
            village_life: k("village_life", 0) as u32,
            carpet_border: k("carpet_border", 0) > 0,
            centre: k("centre", 0),
        }
    }
}

/// The thumper: one mobile strike stack that breaks stalemated borders.
struct StrikeKnobs {
    /// First tick a strike stack may be raised (0 = never).
    from: u32,
    /// Smallest spare stack worth raising as the thumper.
    min: u64,
    /// % of the thumper a trail hex keeps at most when the stack moves on.
    leave_pct: u64,
    /// Score per enemy neighbour of a target (deeper ground is cheaper next tick).
    depth: i64,
    /// Thumper marches to a launch pad at most this far through our land.
    reach: u32,
    /// Spare stacks of at least this many walk toward the thumper on idle ticks (0 = never).
    feed_min: u64,
    /// A border stack may become the thumper when what it can send is at least this %
    /// of a sure breach of its cheapest enemy neighbour (0 = only spare stacks).
    breach_pct: u64,
    /// Keep a rally hex for the feed while no breach is affordable.
    rally: bool,
    /// Score per own neighbour of a thumper target, and per exposed one.
    hug: i64,
    open: i64,
    /// From this tick the thumper hunts the rival with the least known land (0 = never)...
    hunt_from: u32,
    /// ...with this score bonus for its hexes.
    hunt: i64,
}

impl StrikeKnobs {
    fn read(style: &Style) -> StrikeKnobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        StrikeKnobs {
            from: k("strike_from", 0) as u32,
            min: k("strike_min", 3000) as u64,
            leave_pct: k("leave_pct", 15) as u64,
            depth: k("depth", 30),
            reach: k("reach", 8) as u32,
            feed_min: k("feed_min", 0) as u64,
            breach_pct: k("breach_pct", 0) as u64,
            rally: k("rally", 0) > 0,
            hug: k("strike_hug", 0),
            open: k("strike_open", 0),
            hunt_from: k("hunt_from", 0) as u32,
            hunt: k("hunt", 0),
        }
    }
}

pub struct Warren {
    name: String,
    rules: RulesConfig,
    k: Knobs,
    b: BuildKnobs,
    s: StrikeKnobs,
    geo: Option<Geometry>,
    /// Last seen state of every slot.
    seen: Vec<Option<Cell>>,
    /// Slot of the thumper stack, while one is on the move.
    army: Option<usize>,
    tick: u32,
    /// Chance (milli) an attacker wins an exchange this tick; rises in overtime.
    win: u32,
    /// The rival the thumper is finishing off.
    prey: Option<PlayerId>,
    /// Tick each slot first lies in the storm (u32::MAX = never).
    doom: Vec<u32>,
    /// Border hex the feed builds up until it can breach, while no thumper is out.
    rally: Option<usize>,
}

fn kind_of(c: &VisibleHex) -> Option<Building> {
    c.building.as_ref().map(|b| b.kind)
}

impl Warren {
    pub fn new(style: Style) -> Self {
        Warren {
            name: style.id.clone(),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            b: BuildKnobs::read(&style),
            s: StrikeKnobs::read(&style),
            geo: None,
            seen: vec![],
            army: None,
            tick: 0,
            win: RulesConfig::default().attacker_exchange_win_milli,
            doom: vec![],
            prey: None,
            rally: None,
        }
    }

    /// Safe ticks left on slot `i` before the storm reaches it.
    fn life(&self, i: usize) -> u32 {
        self.doom.get(i).map_or(u32::MAX, |d| d.saturating_sub(self.tick))
    }

    fn compute_doom(&mut self, geo: &Geometry) {
        let centre = Hex::new(0, 0);
        let dist: Vec<i32> = geo.hexes.iter().map(|h| h.distance(centre)).collect();
        let floor = (0..geo.passable.len())
            .filter(|&i| geo.passable[i])
            .map(|i| dist[i])
            .min()
            .unwrap_or(0);
        let r = &self.rules;
        let mut by_dist = vec![u32::MAX; (geo.radius + 2) as usize];
        if r.storm_end > r.storm_start {
            for t in r.storm_start..=r.storm_end {
                if let Some(sr) = r.storm_radius(t, geo.radius, floor) {
                    for d in (sr + 1).max(0)..=geo.radius {
                        let e = &mut by_dist[d as usize];
                        if *e == u32::MAX {
                            *e = t;
                        }
                    }
                }
            }
        }
        self.doom = dist.iter().map(|&d| by_dist[d.clamp(0, geo.radius) as usize]).collect();
    }

    /// Evacuates the biggest stack the storm has reached or is about to reach, one hex
    /// toward safer ground (capturing inward when that is a sure thing).
    fn flee(&self, geo: &Geometry, me: PlayerId, mine: &[usize]) -> Option<Action> {
        if self.b.flee_lead == 0 {
            return None;
        }
        let mut best: Option<(u32, usize, usize)> = None;
        for &s in mine {
            let troops = self.seen[s]?.troops;
            if troops < self.b.flee_min.max(2) || self.life(s) > self.b.flee_lead {
                continue;
            }
            let dest = geo.nbrs[s]
                .iter()
                .copied()
                .filter(|&n| self.life(n) > self.life(s))
                .filter(|&n| {
                    self.owned(n, me)
                        || self.seen[n].is_some_and(|c| {
                            let defense = self.defense(geo, s, n, c.wall_milli) as u32;
                            let (mean, sd) = self.loss_stats(c.troops, defense);
                            ((mean + 3 * sd).div_ceil(1000) + 1) < troops as u64
                        })
                })
                .max_by_key(|&n| (self.life(n), self.owned(n, me), std::cmp::Reverse(n)));
            if let Some(d) = dest {
                if best.map_or(true, |b| troops > b.0) {
                    best = Some((troops, s, d));
                }
            }
        }
        let (troops, s, d) = best?;
        Some(Action::Move { from: geo.hexes[s], to: geo.hexes[d], troops: troops - 1 })
    }

    /// Score for ground the storm reaches late, per 1000 safe ticks left.
    fn centre_value(&self, i: usize) -> i64 {
        if self.b.centre == 0 {
            return 0;
        }
        self.b.centre * self.life(i).min(10_000) as i64 / 1000
    }

    /// The living rival we remember the least land of.
    fn find_prey(&self, view: &View) -> Option<PlayerId> {
        let mut land = vec![0u32; view.public_players.len()];
        for c in self.seen.iter().flatten() {
            if let Some(o) = c.owner.filter(|&o| o != view.you) {
                if let Some(l) = land.get_mut(o as usize) {
                    *l += 1;
                }
            }
        }
        view.public_players
            .iter()
            .filter(|p| p.alive && p.id != view.you && land[p.id as usize] > 0)
            .min_by_key(|p| (land[p.id as usize], p.id))
            .map(|p| p.id)
    }

    fn troops_at(&self, i: usize) -> u64 {
        self.seen[i].map_or(0, |c| c.troops as u64)
    }

    /// Troops `s` is short of a sure breach of its cheapest enemy neighbour.
    fn shortfall(&self, geo: &Geometry, me: PlayerId, s: usize) -> Option<i64> {
        let have = self.seen[s]?.troops as i64;
        let mut best: Option<i64> = None;
        for &t in &geo.nbrs[s] {
            let Some(tc) = self.seen[t].filter(|c| c.owner.is_some_and(|o| o != me)) else { continue };
            let defense = self.defense(geo, s, t, tc.wall_milli) as u32;
            let (mean, sd) = self.loss_stats(tc.troops, defense);
            let need = ((mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1) as i64;
            let short = need * self.s.breach_pct.max(100) as i64 / 100 * (100 + self.s.leave_pct as i64) / 100 - have;
            best = Some(best.map_or(short, |b| b.min(short)));
        }
        best
    }

    /// Sticky choice of the border hex closest to affording a breach.
    fn pick_rally(&mut self, geo: &Geometry, me: PlayerId, mine: &[usize]) {
        let cur = self
            .rally
            .filter(|&r| self.owned(r, me))
            .and_then(|r| self.shortfall(geo, me, r).map(|s| (s, r)));
        let best = mine
            .iter()
            .filter_map(|&s| self.shortfall(geo, me, s).map(|v| (v, s)))
            .min();
        self.rally = match (cur, best) {
            (Some(c), Some(b)) if b.0 * 2 < c.0 => Some(b.1),
            (Some(c), _) => Some(c.1),
            (None, b) => b.map(|b| b.1),
        };
    }

    /// Walks the spare stack that adds the most per step one hex toward the thumper.
    fn feed(&self, geo: &Geometry, me: PlayerId) -> Option<Action> {
        let a = self.army.or(self.rally)?;
        if self.s.feed_min == 0 {
            return None;
        }
        let mut prev = vec![usize::MAX; geo.passable.len()];
        let mut dist = vec![0u32; geo.passable.len()];
        prev[a] = a;
        let mut queue = VecDeque::from([a]);
        let mut best: Option<(u64, usize)> = None;
        while let Some(c) = queue.pop_front() {
            if c != a {
                let spare = self.spare(geo, me, c);
                if spare >= self.s.feed_min {
                    let v = spare / dist[c] as u64;
                    if best.map_or(true, |b| v > b.0) {
                        best = Some((v, c));
                    }
                }
            }
            if dist[c] >= self.s.reach {
                continue;
            }
            for &n in &geo.nbrs[c] {
                if prev[n] == usize::MAX && self.owned(n, me) {
                    prev[n] = c;
                    dist[n] = dist[c] + 1;
                    queue.push_back(n);
                }
            }
        }
        let (_, i) = best?;
        let troops = self.spare(geo, me, i) as u32;
        Some(Action::Move { from: geo.hexes[i], to: geo.hexes[prev[i]], troops })
    }

    /// The thumper's move this tick: chain a sure capture into enemy ground, or march
    /// through our land to the launch pad facing the cheapest breach in reach.
    fn strike(&mut self, geo: &Geometry, me: PlayerId, mine: &[usize], tick: u32) -> Option<Action> {
        if self.s.from == 0 || tick < self.s.from {
            return None;
        }
        if let Some(a) = self.army {
            if !self.owned(a, me) || self.troops_at(a) < self.s.min / 2 {
                self.army = None;
            }
        }
        if self.army.is_none() && self.s.breach_pct > 0 {
            let mut best: Option<(u64, usize)> = None;
            for &s in mine {
                let have = self.troops_at(s);
                if have < self.s.min {
                    continue;
                }
                for &t in &geo.nbrs[s] {
                    let Some(tc) = self.seen[t].filter(|c| c.owner.is_some_and(|o| o != me)) else { continue };
                    let defense = self.defense(geo, s, t, tc.wall_milli) as u32;
                    let (mean, sd) = self.loss_stats(tc.troops, defense);
                    let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
                    let leave = self.want(geo, me, s, t).unwrap_or(1).clamp(1, have * self.s.leave_pct / 100);
                    let send = have.saturating_sub(leave);
                    if send * 100 >= need * self.s.breach_pct && best.map_or(true, |b| send - need > b.0) {
                        best = Some((send - need, s));
                    }
                }
            }
            self.army = best.map(|(_, s)| s);
            if self.army.is_none() && self.s.rally {
                self.pick_rally(geo, me, mine);
            } else {
                self.rally = None;
            }
        }
        if self.army.is_none() {
            self.army = mine
                .iter()
                .copied()
                .map(|i| (self.spare(geo, me, i), i))
                .filter(|&(s, _)| s >= self.s.min)
                .max()
                .map(|(_, i)| i);
        }
        let a = self.army?;
        let have = self.troops_at(a);
        let mut best: Option<(i64, usize, u64)> = None;
        for &t in &geo.nbrs[a] {
            let Some(tc) = self.seen[t] else { continue };
            if tc.owner == Some(me) || (self.b.cap_life > 0 && self.life(t) <= self.b.cap_life) {
                continue;
            }
            let defense = self.defense(geo, a, t, tc.wall_milli) as u32;
            let (mean, sd) = self.loss_stats(tc.troops, defense);
            let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
            let leave = self
                .want(geo, me, a, t)
                .unwrap_or(1)
                .clamp(1, have * self.s.leave_pct / 100)
                .max(1);
            if have < leave + need {
                continue;
            }
            let send = have - leave;
            let depth = geo.nbrs[t]
                .iter()
                .filter(|&&n| tc.owner.is_some() && self.seen[n].is_some_and(|c| c.owner == tc.owner))
                .count() as i64;
            let enemy = tc.owner.is_some() as i64;
            let hugged = geo.nbrs[t].iter().filter(|&&n| self.owned(n, me)).count() as i64;
            let exposed = geo.nbrs[t].len() as i64 - hugged;
            let score = self.k.land
                + enemy * (self.k.enemy + self.s.depth * depth + self.k.enclose * self.enclosure_gain(geo, t, me))
                + self.s.hug * hugged
                - self.s.open * exposed
                + self.centre_value(t)
                + if tc.owner.is_some() && tc.owner == self.prey { self.s.hunt } else { 0 }
                - (mean / 1000) as i64 * 100 / (have as i64 / 100).max(1);
            if best.map_or(true, |b| (score, std::cmp::Reverse(t)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((score, t, send));
            }
        }
        if let Some((_, t, send)) = best {
            self.army = Some(t);
            return Some(Action::Move { from: geo.hexes[a], to: geo.hexes[t], troops: send as u32 });
        }
        // Nothing to hit from here: march toward the own hex facing the cheapest breach.
        let mut prev = vec![usize::MAX; geo.passable.len()];
        let mut dist = vec![u32::MAX; geo.passable.len()];
        prev[a] = a;
        dist[a] = 0;
        let mut queue = VecDeque::from([a]);
        let mut pad: Option<(u64, usize)> = None;
        while let Some(c) = queue.pop_front() {
            if c != a {
                for &t in &geo.nbrs[c] {
                    let Some(tc) = self.seen[t].filter(|x| x.owner.is_some_and(|o| o != me)) else { continue };
                    let defense = self.defense(geo, c, t, tc.wall_milli) as u32;
                    let (mean, sd) = self.loss_stats(tc.troops, defense);
                    let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
                    let mut cost = need + dist[c] as u64 * have / 50;
                    if self.s.hunt > 0 && tc.owner == self.prey {
                        cost /= 2;
                    }
                    if need * 2 < have && pad.map_or(true, |p| cost < p.0) {
                        pad = Some((cost, c));
                    }
                }
            }
            if dist[c] >= self.s.reach {
                continue;
            }
            for &n in &geo.nbrs[c] {
                if prev[n] == usize::MAX && self.owned(n, me) {
                    prev[n] = c;
                    dist[n] = dist[c] + 1;
                    queue.push_back(n);
                }
            }
        }
        let Some((_, mut step)) = pad else {
            self.army = None;
            return None;
        };
        while prev[step] != a {
            step = prev[step];
        }
        let leave = self.want(geo, me, a, usize::MAX).unwrap_or(1).clamp(1, have * self.s.leave_pct / 100).max(1);
        if have <= leave + 1 {
            return None;
        }
        self.army = Some(step);
        Some(Action::Move { from: geo.hexes[a], to: geo.hexes[step], troops: (have - leave) as u32 })
    }

    /// Free link commands: Farms adopt adjacent unsupported Villages, adjacent Walls pair up.
    fn management(&self, view: &View) -> Vec<ManagementCommand> {
        let me = view.you;
        let mine: Vec<&VisibleHex> =
            view.visible_hexes.iter().filter(|c| c.owner == Some(me)).collect();
        let at = |h: Hex| mine.iter().find(|c| c.hex == h).copied();
        let mut out = vec![];
        let mut adopted: Vec<Hex> = vec![];
        for farm in mine.iter().filter(|c| kind_of(c) == Some(Building::Farm)) {
            let b = farm.building.as_ref().unwrap();
            if b.link_cooldown > 0 || b.links.len() >= self.rules.farm.sockets as usize {
                continue;
            }
            let village = farm.hex.neighbors().into_iter().find(|&n| {
                at(n).is_some_and(|v| {
                    kind_of(v) == Some(Building::Village)
                        && v.building.as_ref().unwrap().links.is_empty()
                        && !adopted.contains(&n)
                })
            });
            if let Some(v) = village {
                adopted.push(v);
                out.push(ManagementCommand::SetFarmLink { farm: farm.hex, village: v, enabled: true });
            }
        }
        let max = self.rules.wall.max_links as usize;
        let mut used: Vec<(Hex, usize)> = vec![];
        let mut busy: Vec<Hex> = vec![];
        let free = |c: &VisibleHex, used: &[(Hex, usize)]| {
            let b = c.building.as_ref().unwrap();
            let extra = used.iter().filter(|(h, _)| *h == c.hex).count();
            b.link_cooldown == 0 && b.links.len() + extra < max
        };
        let walls: Vec<&VisibleHex> =
            mine.iter().copied().filter(|c| kind_of(c) == Some(Building::Wall)).collect();
        for a in &walls {
            if busy.contains(&a.hex) || !free(a, &used) {
                continue;
            }
            for n in a.hex.neighbors() {
                let Some(b) = walls.iter().find(|w| w.hex == n) else { continue };
                if busy.contains(&b.hex)
                    || !free(b, &used)
                    || a.building.as_ref().unwrap().links.contains(&b.hex)
                {
                    continue;
                }
                out.push(ManagementCommand::SetWallLink { wall_a: a.hex, wall_b: b.hex, enabled: true });
                used.push((a.hex, 1));
                used.push((b.hex, 1));
                busy.push(a.hex);
                busy.push(b.hex);
                break;
            }
        }
        out
    }

    /// The building worth this tick, if any is affordable now.
    fn build(&self, view: &View, geo: &Geometry) -> Option<Action> {
        let me = view.you;
        let tick = view.tick;
        let mine: Vec<&VisibleHex> =
            view.visible_hexes.iter().filter(|c| c.owner == Some(me)).collect();
        let count = |k: Building| mine.iter().filter(|c| kind_of(c) == Some(k)).count();
        let resource = view.own_economy.resource;
        let near_quarry = |h: Hex| {
            h.neighbors().into_iter().any(|n| {
                mine.iter().any(|q| q.hex == n && kind_of(q) == Some(Building::Quarry))
            })
        };
        // Biggest adjacent enemy stack, or None when no enemy touches the hex.
        let heat = |h: Hex| -> Option<u32> {
            let mut worst = None;
            for n in h.neighbors() {
                let Some(i) = geo.slot(n) else { continue };
                if let Some(c) = self.seen[i].filter(|c| c.owner.is_some_and(|o| o != me)) {
                    worst = Some(worst.map_or(c.troops, |w: u32| w.max(c.troops)));
                }
            }
            worst
        };
        let interior = |h: Hex| {
            geo.slot(h).is_some()
                && geo.nbrs[geo.slot(h).unwrap()].iter().all(|&j| self.owned(j, me))
        };
        let height = |h: Hex| geo.slot(h).map_or(0, |i| geo.height[i]);
        let affordable = |c: &VisibleHex, k: Building, extra: u32| {
            let cost = self.rules.build_cost(k, near_quarry(c.hex));
            resource >= cost.resource && c.troops >= cost.troops + 1 + extra
        };
        let life = |h: Hex| geo.slot(h).map_or(0, |i| self.life(i));
        let vl = self.b.village_life;
        let empty = || mine.iter().copied().filter(|c| c.building.is_none());

        let farms = count(Building::Farm);
        if farms < self.b.farms && tick >= self.b.farm_tick {
            let site = empty()
                .filter(|c| self.rules.terrain_allowed(Building::Farm, height(c.hex)))
                .filter(|c| interior(c.hex) && affordable(c, Building::Farm, 0) && life(c.hex) >= 2 * vl)
                .max_by_key(|c| {
                    let room = c
                        .hex
                        .neighbors()
                        .iter()
                        .filter(|n| mine.iter().any(|m| m.hex == **n && m.building.is_none()))
                        .count();
                    (room, c.troops, std::cmp::Reverse(c.hex))
                });
            if let Some(c) = site {
                return Some(Action::Build { at: c.hex, building: Building::Farm });
            }
        }
        if count(Building::Wall) < self.b.walls && tick >= self.b.wall_tick {
            let site = empty()
                .filter_map(|c| heat(c.hex).map(|h| (h, c)))
                .filter(|(h, c)| {
                    *h >= self.b.wall_heat
                        && affordable(c, Building::Wall, self.b.wall_keep)
                        && life(c.hex) >= self.b.cap_life
                })
                .max_by_key(|(h, c)| {
                    let walls = c
                        .hex
                        .neighbors()
                        .iter()
                        .filter(|n| mine.iter().any(|m| m.hex == **n && kind_of(m) == Some(Building::Wall)))
                        .count() as u32;
                    (*h + 50 * walls.min(2), c.troops, std::cmp::Reverse(c.hex))
                });
            if let Some((_, c)) = site {
                return Some(Action::Build { at: c.hex, building: Building::Wall });
            }
        }
        if count(Building::Village) < self.b.villages {
            let site = empty()
                .filter(|c| interior(c.hex) && affordable(c, Building::Village, 0) && life(c.hex) >= vl)
                .find(|c| {
                    c.hex.neighbors().iter().any(|n| {
                        mine.iter().any(|f| {
                            f.hex == *n
                                && kind_of(f) == Some(Building::Farm)
                                && f.building.as_ref().unwrap().links.len()
                                    < self.rules.farm.sockets as usize
                        })
                    })
                });
            if let Some(c) = site {
                return Some(Action::Build { at: c.hex, building: Building::Village });
            }
        }
        if count(Building::Quarry) < self.b.quarries && tick >= self.b.quarry_tick {
            let min = self.rules.quarry.min_friendly_distance;
            let site = empty()
                .filter(|c| self.rules.terrain_allowed(Building::Quarry, height(c.hex)))
                .filter(|c| interior(c.hex) && affordable(c, Building::Quarry, 0) && life(c.hex) >= 2 * vl)
                .filter(|c| {
                    !mine.iter().any(|q| kind_of(q) == Some(Building::Quarry) && q.hex.distance(c.hex) < min)
                })
                .max_by_key(|c| (c.troops, std::cmp::Reverse(c.hex)));
            if let Some(c) = site {
                return Some(Action::Build { at: c.hex, building: Building::Quarry });
            }
        }
        let village_cost = self.rules.build_cost(Building::Village, false).resource;
        if tick >= self.b.carpet_tick && self.b.carpet_tick > 0 && resource >= village_cost + self.b.carpet_reserve.min(village_cost * 5) {
            let site = empty()
                .filter(|c| (self.b.carpet_border || interior(c.hex)) && affordable(c, Building::Village, 0))
                .filter(|c| life(c.hex) >= vl)
                .max_by_key(|c| (life(c.hex).min(20_000) / 100, c.troops, std::cmp::Reverse(c.hex)));
            if let Some(c) = site {
                return Some(Action::Build { at: c.hex, building: Building::Village });
            }
        }
        None
    }

    /// Mean and standard deviation (both x1000) of the attackers lost taking `def`
    /// troops behind `defense` milli, from the engine's exchange loop.
    fn loss_stats(&self, def: u32, defense: u32) -> (u64, u64) {
        let d = defense.max(1) as u64;
        let per_win = (1_000_000 + d / 2) / d;
        let wins = (def as u64 * 1000).div_ceil(per_win);
        let p = self.win as u64;
        let q = 1000 - p;
        let mean = wins * q * 1000 / p;
        // Negative binomial: var = wins * q / p^2.
        let var_x1e6 = wins * q * 1_000_000_000 / (p * p);
        (mean, isqrt(var_x1e6))
    }

    /// Defenders `attackers` troops expect to kill behind `defense` milli going all in.
    fn kills(&self, attackers: u64, defense: u64) -> u64 {
        let p = self.win as u64;
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
        if self.b.flee_lead > 0 && self.life(h) <= self.b.flee_lead {
            return None;
        }
        let mut worst: Option<u64> = None;
        // A Wall we take loses its links, so it only counts at its bare multiplier.
        let wall = match self.seen[h] {
            Some(c) if self.b.wall_aware && c.wall_milli > MILLI => {
                if c.owner == Some(me) {
                    c.wall_milli
                } else {
                    self.rules.wall_multiplier_milli(0)
                }
            }
            _ => MILLI,
        };
        for &n in &geo.nbrs[h] {
            if n == ours || !self.enemy(n, me) {
                continue;
            }
            let stack = self.seen[n].unwrap().troops.saturating_sub(1) as u64;
            let k = self.kills(stack, self.defense(geo, n, h, wall));
            worst = Some(worst.map_or(k, |w| w.max(k)));
        }
        let late = if self.b.guard_late > 0 && self.tick > self.b.guard_from {
            let ramp = (self.tick - self.b.guard_from) as u64;
            (self.b.guard_late * ramp / self.b.guard_ramp.max(1) as u64).min(self.b.guard_late) * MILLI as u64
                / wall.max(MILLI) as u64
        } else {
            0
        };
        worst.map(|k| self.k.guard.max(late) + k * self.k.margin_pct / 100)
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

    /// Articulation points of the land we don't own (Tarjan, iterative). Enemy hexes
    /// root the search first, so regions that hold an enemy are never a backyard.
    fn seal(&self, geo: &Geometry, me: PlayerId) -> Seal {
        let slots = geo.passable.len();
        let open = |i: usize| geo.passable[i] && !self.owned(i, me);
        let mut cuts = vec![0u32; slots];
        let mut pocket = vec![false; slots];
        let mut disc = vec![0u32; slots];
        let mut low = vec![0u32; slots];
        let mut size = vec![1u32; slots];
        let mut parent = vec![usize::MAX; slots];
        let mut timer = 0u32;
        let mut roots: Vec<usize> = (0..slots).filter(|&i| open(i) && self.enemy(i, me)).collect();
        roots.extend((0..slots).filter(|&i| open(i) && !self.enemy(i, me)));
        let mut stack: Vec<(usize, usize)> = vec![];
        let mut comp: Vec<usize> = vec![];
        for root in roots {
            if disc[root] != 0 {
                continue;
            }
            comp.clear();
            let mut has_enemy = false;
            timer += 1;
            disc[root] = timer;
            low[root] = timer;
            stack.push((root, 0));
            while let Some(top) = stack.last_mut() {
                let (v, it) = *top;
                if it < geo.nbrs[v].len() {
                    top.1 += 1;
                    let w = geo.nbrs[v][it];
                    if !open(w) {
                        continue;
                    }
                    if disc[w] == 0 {
                        parent[w] = v;
                        timer += 1;
                        disc[w] = timer;
                        low[w] = timer;
                        stack.push((w, 0));
                    } else if w != parent[v] {
                        low[v] = low[v].min(disc[w]);
                    }
                    continue;
                }
                stack.pop();
                comp.push(v);
                has_enemy |= self.enemy(v, me);
                if v == root {
                    continue;
                }
                let p = parent[v];
                low[p] = low[p].min(low[v]);
                size[p] += size[v];
                if low[v] >= disc[p] && size[v] <= self.k.seal_max && p != root {
                    cuts[p] += size[v];
                }
            }
            if !has_enemy && comp.len() as u32 <= self.k.seal_max {
                for &c in &comp {
                    pocket[c] = true;
                    cuts[c] = 0;
                }
            }
        }
        Seal { cuts, pocket }
    }

    /// The best capture. While `sprinting` nothing needs to be held past the cutoff, so
    /// every capture sends just enough to be sure and safety is ignored.
    fn best_capture(
        &self,
        geo: &Geometry,
        me: PlayerId,
        mine: &[usize],
        sprinting: bool,
        seal: Option<&Seal>,
    ) -> (Option<Capture>, Option<Capture>) {
        let mut best: Option<Capture> = None;
        // Best capture the pad can't afford yet; `send` holds the troops it is short.
        let mut blocked: Option<Capture> = None;
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
                if tc.owner == Some(me) || (self.b.cap_life > 0 && self.life(t) <= self.b.cap_life) {
                    continue;
                }
                let defense = self.defense(geo, s, t, tc.wall_milli) as u32;
                let (mean, sd) = self.loss_stats(tc.troops, defense);
                let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
                let loss = mean.div_ceil(1000);
                let want_t = self.want(geo, me, t, s);
                if need > avail {
                    if self.k.stage > 0 {
                        let full = need.max(want_t.map_or(0, |w| loss + w));
                        let short = full + 1 - troops.min(full + 1);
                        let score = self.capture_gain(geo, me, t, tc, seal)
                            - loss as i64 * cost_milli / 1000
                            - short as i64 * self.k.short_cost;
                        if blocked.map_or(true, |b| (score, std::cmp::Reverse(t)) > (b.score, std::cmp::Reverse(b.to))) {
                            blocked = Some(Capture { score, from: s, to: t, send: short as u32 });
                        }
                    }
                    continue;
                }
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
                if sprinting {
                    pick = (1_000_000, 1000, need);
                }
                let (v, st, send) = pick;
                if st < self.k.safe_min {
                    continue;
                }
                let gain = self.capture_gain(geo, me, t, tc, seal);
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
        (best, blocked)
    }

    /// Score of holding `t` once taken, before troop costs.
    fn capture_gain(&self, geo: &Geometry, me: PlayerId, t: usize, tc: Cell, seal: Option<&Seal>) -> i64 {
        let hugged = geo.nbrs[t].iter().filter(|&&n| self.owned(n, me)).count() as i64;
        let exposed = geo.nbrs[t].len() as i64 - hugged;
        let mut gain = self.k.land;
        if tc.owner.is_some() {
            gain += self.k.enemy + self.k.enclose * self.enclosure_gain(geo, t, me);
        }
        gain += self.k.hug * hugged - self.k.open * exposed + self.k.height * geo.height[t] as i64;
        gain += self.centre_value(t);
        if let Some(seal) = seal {
            gain += self.k.seal * seal.cuts[t] as i64;
            if seal.pocket[t] {
                gain -= self.k.backyard;
            }
        }
        gain
    }

    /// Walks the nearest spare stack one step toward `pad` (or gathers onto it while
    /// nobody touches our land early on).
    fn stage_to(&self, geo: &Geometry, me: PlayerId, pad: usize, calm: bool) -> Option<Action> {
        if calm {
            return Some(Action::Gather { target: geo.hexes[pad] });
        }
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
        // Once enemies touch us a Gather would strip every far border hex down to one
        // troop, so stage single stacks instead.
        self.stage_to(geo, me, pad, !contact && tick < self.k.gather_until)
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

impl Bot for Warren {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        self.tick = view.tick;
        self.win = self.rules.attacker_win_milli(view.tick);
        let geo = self.geo.take().unwrap_or_else(|| Geometry::new(view));
        if self.doom.is_empty() {
            self.compute_doom(&geo);
        }
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
        let sprinting = view.tick + self.k.sprint >= self.rules.max_ticks;
        let seal = (self.k.seal > 0 || self.k.backyard > 0).then(|| self.seal(&geo, me));
        let supply = self.supply(&geo, me, &mine);
        let (capture, blocked) = self.best_capture(&geo, me, &mine, sprinting, seal.as_ref());
        let capture = capture.filter(|c| sprinting || c.score >= self.k.min_score);
        let supply_tick =
            !sprinting && self.k.supply_every > 0 && view.tick % self.k.supply_every == 0;
        let floor = capture.map_or(self.k.min_score, |c| c.score);
        let staging = blocked
            .filter(|b| !sprinting && mine.len() > 1 && b.score >= floor + self.k.stage)
            .and_then(|b| {
                let contact =
                    mine.iter().any(|&s| geo.nbrs[s].iter().any(|&n| self.enemy(n, me)));
                self.stage_to(&geo, me, b.from, !contact && view.tick < self.k.gather_until)
            });
        let weak_capture = capture.map_or(true, |c| c.score < self.k.min_score + self.b.preempt_below);
        let build = (!sprinting && weak_capture && staging.is_none())
            .then(|| self.build(view, &geo))
            .flatten();
        self.prey = (self.s.hunt_from > 0 && view.tick >= self.s.hunt_from)
            .then(|| self.find_prey(view))
            .flatten();
        let flee = self.flee(&geo, me, &mine);
        let strike = if flee.is_some() { None } else { self.strike(&geo, me, &mine, view.tick) };
        let feed = if sprinting { None } else { self.feed(&geo, me) };
        let action = match (supply, capture) {
            _ if flee.is_some() => flee.unwrap(),
            _ if strike.is_some() => strike.unwrap(),
            (Some((gain, a)), _) if supply_tick && gain >= self.k.supply_min => a,
            _ if staging.is_some() => staging.unwrap(),
            _ if build.is_some() => build.unwrap(),
            (_, Some(c)) => {
                Action::Move { from: geo.hexes[c.from], to: geo.hexes[c.to], troops: c.send }
            }
            _ if feed.is_some() => feed.unwrap(),
            (Some((gain, a)), None) if gain > 0 => a,
            _ => self.regroup(&geo, me, &mine, view.tick).unwrap_or(Action::NoOp),
        };
        let management = self.management(view);
        self.geo = Some(geo);
        PlayerCommand { management, action }
    }
}
