//! Pangolin: Ridgeline's border play on top of a Farm-Village lattice.
//!
//! Colour every hex by `(q - r) mod 3`: no two neighbours share a colour, and each hex
//! has three neighbours of each other colour. Villages go on one colour and Farms on the
//! other two, so in a finished patch every Village touches six Farms (8 troops a tick)
//! and every Farm boosts three Villages. The colour is picked once, from the land around
//! our start, to put Villages on hills (no Farm can stand there) and Farms on low ground.
//! Only hexes at least `depth` steps behind our border are built on, and each build is
//! ranked by the troops per tick it adds per troop it costs, counting the lattice
//! neighbours it will feed later at `future_pct`. A Quarry goes on a hill whose
//! neighbours are mostly unbuilt sites, when its discount pays for it.
//!
//! The tick goes to a build instead of Ridgeline's move when that move isn't a capture,
//! or when the build pays back faster than `rush_roi`.

use super::apex_core::Ridgeline;
use crate::{Action, Bot, Building, Hex, ManagementCommand, PlayerCommand, PlayerId, RulesConfig, Style, View};
use std::collections::VecDeque;

struct Map {
    radius: i32,
    width: i32,
    passable: Vec<bool>,
    height: Vec<i8>,
    hexes: Vec<Hex>,
    nbrs: Vec<Vec<usize>>,
    /// Touches the map edge or impassable ground, so no region holding it can be enclosed.
    open_edge: Vec<bool>,
}

impl Map {
    fn new(view: &View) -> Map {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let mut passable = vec![false; slots];
        let mut height = vec![0i8; slots];
        let hexes: Vec<Hex> =
            (0..slots).map(|i| Hex::new(i as i32 % width - radius, i as i32 / width - radius)).collect();
        for i in 0..slots {
            if let Some(t) = view.public_terrain[i] {
                passable[i] = t.passable();
                height[i] = t.height;
            }
        }
        let mut m = Map { radius, width, passable, height, hexes, nbrs: vec![], open_edge: vec![] };
        m.nbrs = (0..slots)
            .map(|i| {
                m.hexes[i].neighbors().iter().filter_map(|n| m.slot(*n)).filter(|&j| m.passable[j]).collect()
            })
            .collect();
        m.open_edge = (0..slots).map(|i| m.passable[i] && m.nbrs[i].len() < 6).collect();
        m
    }

    fn slot(&self, h: Hex) -> Option<usize> {
        let r = self.radius;
        (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
            .then(|| ((h.r + r) * self.width + h.q + r) as usize)
    }

    fn colour(&self, i: usize) -> i32 {
        let h = self.hexes[i];
        (h.q - h.r).rem_euclid(3)
    }
}

struct Knobs {
    /// No builds before this tick.
    build_from: u32,
    /// No builds from this tick on (0 = build all game).
    build_until: u32,
    /// Fewest ticks between two builds.
    build_gap: u32,
    /// Fewest steps from a build site to land we don't own.
    depth: u32,
    /// % of a troop a tick credited for each empty lattice partner a build will feed.
    future_pct: u64,
    /// Builds paying back less than this (troops a tick per 1000 troops) are skipped.
    min_roi: u64,
    /// Builds paying back at least this take the tick even from a capture.
    rush_roi: u64,
    /// Quarries on hills (0/1).
    quarry: bool,
    /// Lattice sites (x10 troops saved per troop paid) a Quarry must discount to be built.
    quarry_min_x10: u64,
    /// Troops a tick a Quarry adds to each adjacent Village (the frozen binary can't read
    /// it from newer live rules, so it is a knob).
    quarry_village: u64,
    /// Walls on enemy-facing border hexes from this tick (0 = never).
    wall_from: u32,
    /// Fewest ticks between two Walls.
    wall_gap: u32,
    /// Most Walls to own.
    wall_cap: usize,
    /// Troops a border hex keeps on top of a Wall's cost before it may build one.
    wall_keep: u32,
}

impl Knobs {
    fn read(style: &Style) -> Knobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Knobs {
            build_from: k("build_from", 200) as u32,
            build_until: k("build_until", 0) as u32,
            build_gap: k("build_gap", 2) as u32,
            depth: k("depth", 2) as u32,
            future_pct: k("future_pct", 50) as u64,
            min_roi: k("min_roi", 10) as u64,
            rush_roi: k("rush_roi", 60) as u64,
            quarry: k("quarry", 0) > 0,
            quarry_min_x10: k("quarry_min_x10", 20) as u64,
            quarry_village: k("quarry_village", 0) as u64,
            wall_from: k("wall_from", 0) as u32,
            wall_gap: k("wall_gap", 4) as u32,
            wall_cap: k("wall_cap", 60) as usize,
            wall_keep: k("wall_keep", 10) as u32,
        }
    }
}

pub struct Pangolin {
    inner: Ridgeline,
    rules: RulesConfig,
    k: Knobs,
    map: Option<Map>,
    /// Lattice colour that holds Villages.
    vclass: Option<i32>,
    last_build: Option<u32>,
    last_wall: Option<u32>,
    army: ArmyKnobs,
    state: ArmyState,
    /// % of a sure capture's need sent on a tick the core would NoOp (0 = keep the NoOp).
    breakout_pct: u64,
    /// Fortify once we hold at most this many hexes (0 = never), from `zombie_from` on.
    zombie_land: usize,
    zombie_from: u32,
    zombie_ahead: u32,
    /// Optional work is skipped once a move has taken this long (microseconds); the
    /// capture search stops at half of it.
    budget_us: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum Site {
    Empty,
    Mine(Building),
    Other,
}

impl Pangolin {
    pub fn new(style: Style) -> Pangolin {
        Pangolin {
            inner: Ridgeline::new(style.clone()),
            rules: RulesConfig::default(),
            k: Knobs::read(&style),
            map: None,
            vclass: None,
            last_build: None,
            last_wall: None,
            army: ArmyKnobs::read(&style),
            state: ArmyState::default(),
            breakout_pct: style.params.knob("breakout_pct", 0).max(0) as u64,
            zombie_land: style.params.knob("zombie_land", 0).max(0) as usize,
            zombie_from: style.params.knob("zombie_from", 1500).max(0) as u32,
            zombie_ahead: style.params.knob("zombie_ahead", 1000).max(0) as u32,
            budget_us: style.params.knob("budget_us", 2500).max(100) as u64,
        }
    }

    fn farmable(&self, m: &Map, i: usize) -> bool {
        self.rules.terrain_allowed(Building::Farm, m.height[i])
    }

    fn quarry_ok(&self, m: &Map, i: usize) -> bool {
        self.rules.terrain_allowed(Building::Quarry, m.height[i])
    }

    /// The colour whose hexes near `start` make the best Village sites.
    fn pick_class(&self, m: &Map, start: usize) -> i32 {
        let mut best = (i64::MIN, 0);
        for c in 0..3 {
            let mut score = 0i64;
            for i in 0..m.passable.len() {
                if !m.passable[i] || m.colour(i) != c || m.hexes[i].distance(m.hexes[start]) > 6 {
                    continue;
                }
                score += m.nbrs[i].iter().filter(|&&n| self.farmable(m, n)).count() as i64;
                if !self.farmable(m, i) {
                    score += 3;
                }
            }
            if score > best.0 {
                best = (score, c);
            }
        }
        best.1
    }

    fn build(&mut self, view: &View, m: &Map, vclass: i32) -> Option<(u64, Action)> {
        let me = view.you;
        let slots = m.passable.len();
        let mut owner: Vec<Option<PlayerId>> = vec![None; slots];
        let mut site = vec![Site::Other; slots];
        let mut troops = vec![0u32; slots];
        for c in &view.visible_hexes {
            let Some(i) = m.slot(c.hex) else { continue };
            owner[i] = c.owner;
            troops[i] = c.troops;
            if c.owner == Some(me) {
                site[i] = c.building.as_ref().map_or(Site::Empty, |b| Site::Mine(b.kind));
            }
        }
        // Steps from land we don't own, through our own land.
        let mut depth = vec![u32::MAX; slots];
        let mut queue = VecDeque::new();
        for i in 0..slots {
            if m.passable[i] && owner[i] != Some(me) {
                depth[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(c) = queue.pop_front() {
            for &n in &m.nbrs[c] {
                if depth[n] == u32::MAX {
                    depth[n] = depth[c] + 1;
                    queue.push_back(n);
                }
            }
        }
        let near_quarry = |i: usize| m.nbrs[i].iter().any(|&n| site[n] == Site::Mine(Building::Quarry));
        let cost = |b: Building, i: usize| self.rules.build_cost(b, near_quarry(i)).troops;
        let fut = self.k.future_pct * 10;
        let mut best: Option<(u64, usize, Building)> = None;
        for i in 0..slots {
            if site[i] != Site::Empty || depth[i] < self.k.depth || depth[i] == u32::MAX {
                continue;
            }
            let count = |b: Building| m.nbrs[i].iter().filter(|&&n| site[n] == Site::Mine(b)).count() as u64;
            let mut options: Vec<(Building, u64)> = vec![];
            if m.colour(i) == vclass {
                let partners =
                    m.nbrs[i].iter().filter(|&&n| site[n] == Site::Empty && self.farmable(m, n)).count() as u64;
                let rate = 1000 * (self.rules.village.bonus as u64
                    + self.rules.village.farm_bonus as u64 * count(Building::Farm)
                    + self.k.quarry_village * count(Building::Quarry))
                    + fut * partners;
                options.push((Building::Village, rate));
            } else if self.farmable(m, i) {
                let partners =
                    m.nbrs[i].iter().filter(|&&n| site[n] == Site::Empty && m.colour(n) == vclass).count() as u64;
                let rate = 1000 * self.rules.village.farm_bonus as u64 * count(Building::Village) + fut * partners;
                options.push((Building::Farm, rate));
            } else if self.k.quarry && self.quarry_ok(m, i) {
                let close = (0..slots).any(|j| {
                    site[j] == Site::Mine(Building::Quarry)
                        && m.hexes[j].distance(m.hexes[i]) < self.rules.quarry.min_friendly_distance
                });
                if !close {
                    let saved: u64 = m.nbrs[i]
                        .iter()
                        .filter(|&&n| site[n] == Site::Empty)
                        .map(|&n| {
                            let b = if m.colour(n) == vclass { Building::Village } else { Building::Farm };
                            if b == Building::Farm && !self.farmable(m, n) {
                                return 0;
                            }
                            let c = self.rules.build_cost(b, true);
                            (c.base_troops - c.troops) as u64
                        })
                        .sum();
                    let c = cost(Building::Quarry, i) as u64;
                    let qv = self.k.quarry_village;
                    if qv > 0 {
                        let vsites = m.nbrs[i]
                            .iter()
                            .filter(|&&n| site[n] == Site::Empty && m.colour(n) == vclass)
                            .count() as u64;
                        // A troop saved on a later build counts like a troop a tick for 100 ticks.
                        let rate = 1000 * qv * count(Building::Village) + fut * qv * vsites + saved * 10;
                        options.push((Building::Quarry, rate));
                    } else if saved * 10 >= c * self.k.quarry_min_x10 {
                        options.push((Building::Quarry, self.k.rush_roi * c));
                    }
                }
            }
            for (b, rate) in options {
                let c = cost(b, i);
                if troops[i] < c + 1 {
                    continue;
                }
                let roi = rate / c.max(1) as u64;
                if roi >= self.k.min_roi && best.is_none_or(|x| (roi, std::cmp::Reverse(i)) > (x.0, std::cmp::Reverse(x.1))) {
                    best = Some((roi, i, b));
                }
            }
        }
        best.map(|(roi, i, b)| (roi, Action::Build { at: m.hexes[i], building: b }))
    }

    /// A Wall on the enemy-facing border hex under the biggest adjacent enemy stack,
    /// preferring hexes next to our Walls that still have a free link.
    fn wall(&self, view: &View, m: &Map) -> Option<Action> {
        let me = view.you;
        let slots = m.passable.len();
        let mut cell = vec![None; slots];
        for c in &view.visible_hexes {
            if let Some(i) = m.slot(c.hex) {
                cell[i] = Some(c);
            }
        }
        let is_wall = |c: &crate::VisibleHex| c.building.as_ref().is_some_and(|b| b.kind == Building::Wall);
        let walls = cell.iter().flatten().filter(|c| c.owner == Some(me) && is_wall(c)).count();
        if walls >= self.k.wall_cap {
            return None;
        }
        let max_links = self.rules.wall.max_links as usize;
        let mut best: Option<(u64, usize)> = None;
        for i in 0..slots {
            let Some(c) = cell[i] else { continue };
            if c.owner != Some(me) || c.building.is_some() {
                continue;
            }
            let near_quarry = m.nbrs[i].iter().any(|&n| {
                cell[n].is_some_and(|x| x.owner == Some(me) && x.building.as_ref().is_some_and(|b| b.kind == Building::Quarry))
            });
            let cost = self.rules.build_cost(Building::Wall, near_quarry).troops;
            if c.troops < cost + 1 + self.k.wall_keep {
                continue;
            }
            let mut threat = 0u64;
            let mut linkable = 0u64;
            for &n in &m.nbrs[i] {
                let Some(x) = cell[n] else { continue };
                match x.owner {
                    Some(o) if o != me => threat = threat.max(x.troops as u64),
                    Some(_) if is_wall(x) && x.building.as_ref().unwrap().links.len() < max_links => linkable += 1,
                    _ => {}
                }
            }
            if threat == 0 {
                continue;
            }
            let score = linkable * 1_000_000 + threat;
            if best.is_none_or(|b| (score, std::cmp::Reverse(i)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((score, i));
            }
        }
        best.map(|(_, i)| Action::Build { at: m.hexes[i], building: Building::Wall })
    }

    /// Links between adjacent Walls of ours that both have a free link and no cooldown.
    fn links(&self, view: &View, m: &Map) -> Vec<ManagementCommand> {
        let me = view.you;
        let max_links = self.rules.wall.max_links as usize;
        let mut walls: Vec<(usize, Hex, usize, Vec<Hex>)> = vec![];
        for c in &view.visible_hexes {
            let Some(b) = c.building.as_ref().filter(|b| b.kind == Building::Wall && c.owner == Some(me)) else { continue };
            if b.link_cooldown == 0 {
                walls.push((m.slot(c.hex).unwrap_or(0), c.hex, b.links.len(), b.links.clone()));
            }
        }
        walls.sort_by_key(|w| w.0);
        let mut out = vec![];
        for a in 0..walls.len() {
            for b in a + 1..walls.len() {
                if walls[a].2 >= max_links {
                    break;
                }
                if walls[b].2 >= max_links || walls[a].1.distance(walls[b].1) != 1 || walls[a].3.contains(&walls[b].1) {
                    continue;
                }
                out.push(ManagementCommand::SetWallLink { wall_a: walls[a].1, wall_b: walls[b].1, enabled: true });
                // A link puts both Walls on cooldown, so each takes part in one link a tick.
                walls[a].2 = max_links;
                walls[b].2 = max_links;
            }
        }
        out
    }
}

impl Bot for Pangolin {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if let Some(r) = &view.rules {
            self.rules = r.clone();
        }
        let prof = std::env::var("APEX_PROF").is_ok();
        let t0 = cpu_now();
        let started = std::time::Instant::now();
        self.inner.deadline = Some(started + std::time::Duration::from_micros(self.budget_us / 2));
        let budget_us = self.budget_us;
        let late = |t: std::time::Instant| t.elapsed().as_micros() as u64 > budget_us;
        let mut cmd = self.inner.act(view);
        let t_inner = cpu_now() - t0;
        let m = self.map.take().unwrap_or_else(|| Map::new(view));
        if self.vclass.is_none() {
            let start = view.visible_hexes.iter().find(|c| c.owner == Some(view.you)).and_then(|c| m.slot(c.hex));
            self.vclass = start.map(|s| self.pick_class(&m, s));
        }
        let open = view.tick >= self.k.build_from
            && (self.k.build_until == 0 || view.tick < self.k.build_until)
            && self.last_build.is_none_or(|t| view.tick >= t + self.k.build_gap);
        let capture = match &cmd.action {
            Action::Move { to, .. } => {
                view.visible_hexes.iter().find(|c| c.hex == *to).is_some_and(|c| c.owner != Some(view.you))
            }
            Action::Gather { .. } | Action::Build { .. } | Action::NoOp => false,
        };
        let mut built = false;
        let t1 = cpu_now();
        if let (true, Some(vclass), false) = (open, self.vclass, late(started)) {
            if let Some((roi, a)) = self.build(view, &m, vclass) {
                if !capture || roi >= self.k.rush_roi {
                    cmd.action = a;
                    self.last_build = Some(view.tick);
                    built = true;
                }
            }
        }
        let t_build = cpu_now() - t1;
        let walls = self.k.wall_from > 0 && view.tick >= self.k.wall_from;
        if walls && !built && !capture && !late(started) && self.last_wall.is_none_or(|t| view.tick >= t + self.k.wall_gap) {
            if let Some(a) = self.wall(view, &m) {
                cmd.action = a;
                self.last_wall = Some(view.tick);
            }
        }
        if walls {
            cmd.management.extend(self.links(view, &m));
        }
        if self.army.from > 0 && view.tick >= self.army.from && !late(started) {
            if let Some(a) = self.army_action(view, &m) {
                cmd.action = a;
            }
        }
        if self.zombie_land > 0 && view.tick >= self.zombie_from && !late(started) {
            let capture_now = match &cmd.action {
                Action::Move { to, .. } => {
                    view.visible_hexes.iter().find(|c| c.hex == *to).is_some_and(|c| c.owner != Some(view.you))
                }
                _ => false,
            };
            if !capture_now {
                if let Some(a) = self.fortify(view, &m) {
                    cmd.action = a;
                }
            }
        }
        if self.breakout_pct > 0 && cmd.action == Action::NoOp && !late(started) {
            if let Some(a) = self.breakout(view, &m) {
                cmd.action = a;
            }
        }
        if prof && (cpu_now() - t0).as_micros() > 1000 {
            let mine = view.visible_hexes.iter().filter(|c| c.owner == Some(view.you)).count();
            eprintln!("PROF t{} seat{} land{} vis{} total {:?} inner {:?} build {:?}", view.tick, view.you, mine, view.visible_hexes.len(), (cpu_now() - t0), t_inner, t_build);
        }
        self.map = Some(m);
        cmd
    }
}

/// Late-game army: once attacker odds favour the attack, Gather the interior bank onto
/// one border hex, then walk that stack through enemy land a hex a tick, leaving a
/// garrison on every hex it takes so the trail stays ours.
struct ArmyKnobs {
    /// First tick of army play (0 = never).
    from: u32,
    /// Most Gathers in one mobilization.
    gathers: u32,
    /// A mobilization ends once the army holds this % of our troops.
    ready_pct: u64,
    /// Garrison left behind: the army's troops / `keep_div`, clamped to the two below.
    keep_div: u64,
    keep_min: u64,
    keep_max: u64,
    /// Mobilize again when the army falls under this % of our troops...
    regather_pct: u64,
    /// ...but no sooner than this many ticks after the last mobilization began.
    regather_gap: u32,
    /// Standard deviations (x10) of bad dice an army capture must survive.
    sure_x10: u64,
    /// Army play only while at most this many players are alive.
    alive: u32,
    /// Our hexes a hex must touch for the army to step onto it, unless it lies against
    /// the map edge or impassable ground (where it can never be enclosed).
    hug_min: u32,
    /// Score per rival hex a capture makes surrender.
    pocket: i64,
}

impl ArmyKnobs {
    fn read(style: &Style) -> ArmyKnobs {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        ArmyKnobs {
            from: k("army_from", 0) as u32,
            gathers: k("army_gathers", 20) as u32,
            ready_pct: k("army_ready_pct", 60) as u64,
            keep_div: k("army_keep_div", 200).max(1) as u64,
            keep_min: k("army_keep_min", 300) as u64,
            keep_max: k("army_keep_max", 3000) as u64,
            regather_pct: k("army_regather_pct", 15) as u64,
            regather_gap: k("army_regather_gap", 300) as u32,
            sure_x10: k("army_sure_x10", 30) as u64,
            alive: k("army_alive", 8) as u32,
            hug_min: k("army_hug", 2) as u32,
            pocket: k("army_pocket", 100),
        }
    }
}

#[derive(Default)]
struct ArmyState {
    /// Gather target and Gathers left in the current mobilization.
    rally: Option<(usize, u32)>,
    last_rally: Option<u32>,
}

impl Pangolin {
    /// A cornered seat (at most `zombie_land` hexes) outlives the others by spreading its
    /// bank over its threatened hexes: troops move from the hex with the most to spare to
    /// the neighbouring hex furthest short of what the biggest adjacent enemy stack could
    /// kill at the odds `zombie_odds_ahead` ticks from now.
    fn fortify(&self, view: &View, m: &Map) -> Option<Action> {
        let me = view.you;
        let slots = m.passable.len();
        let mut owner: Vec<Option<PlayerId>> = vec![None; slots];
        let mut troops = vec![0u32; slots];
        let mut wall = vec![1000u32; slots];
        let mut mine = vec![];
        for c in &view.visible_hexes {
            let Some(i) = m.slot(c.hex) else { continue };
            owner[i] = c.owner;
            troops[i] = c.troops;
            wall[i] = c.wall_milli;
            if c.owner == Some(me) {
                mine.push(i);
            }
        }
        if mine.len() > self.zombie_land || mine.len() < 2 {
            return None;
        }
        let p = self.rules.attacker_win_milli(view.tick + self.zombie_ahead) as u64;
        // Troops an all-in enemy stack next to `h` could kill, with a 25% margin.
        let need = |h: usize| -> u64 {
            let mut worst = 0u64;
            for &n in &m.nbrs[h] {
                if owner[n].is_none_or(|o| o == me) {
                    continue;
                }
                let hm = self.rules.height_multiplier_milli(m.height[n], m.height[h]) as u64;
                let d = (hm * wall[h].max(1000) as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64);
                let k = (troops[n].saturating_sub(1)) as u64 * p * 1000 / ((1000 - p) * d.max(1));
                worst = worst.max(k);
            }
            if worst == 0 { 0 } else { worst * 5 / 4 + 20 }
        };
        let needs: Vec<(usize, u64)> = mine.iter().map(|&h| (h, need(h))).collect();
        let need_of = |h: usize| needs.iter().find(|x| x.0 == h).map_or(0, |x| x.1);
        let mut best: Option<(u64, usize, usize, u64)> = None;
        for &(h, nh) in &needs {
            let have = troops[h] as u64;
            if have >= nh {
                continue;
            }
            let short = nh - have;
            for &d in &m.nbrs[h] {
                if owner[d] != Some(me) {
                    continue;
                }
                let spare = (troops[d] as u64).saturating_sub(need_of(d).max(1));
                if spare < 2 {
                    continue;
                }
                let give = spare.min(short + short / 4 + 1);
                // Fill the hex furthest short, relative to what it needs.
                let key = short.min(spare) * 1000 / nh.max(1);
                if best.is_none_or(|b| key > b.0) {
                    best = Some((key, d, h, give));
                }
            }
        }
        if let Some((_, d, h, give)) = best {
            return Some(Action::Move { from: m.hexes[d], to: m.hexes[h], troops: give as u32 });
        }
        None
    }

    /// A tick the core would waste: take the cheapest sure capture from our biggest stack
    /// next to it, sending `breakout_pct` % of what it needs (at most all but one troop).
    fn breakout(&self, view: &View, m: &Map) -> Option<Action> {
        let me = view.you;
        let slots = m.passable.len();
        let mut owner: Vec<Option<PlayerId>> = vec![None; slots];
        let mut troops = vec![0u32; slots];
        let mut wall = vec![1000u32; slots];
        for c in &view.visible_hexes {
            let Some(i) = m.slot(c.hex) else { continue };
            owner[i] = c.owner;
            troops[i] = c.troops;
            wall[i] = c.wall_milli;
        }
        let win = self.rules.attacker_win_milli(view.tick) as u64;
        let mut best: Option<(u64, u64, usize, usize, u64)> = None;
        for s in 0..slots {
            if owner[s] != Some(me) || troops[s] < 2 {
                continue;
            }
            let avail = troops[s] as u64 - 1;
            for &t in &m.nbrs[s] {
                if owner[t] == Some(me) {
                    continue;
                }
                let h = self.rules.height_multiplier_milli(m.height[s], m.height[t]) as u64;
                let defense = (h * wall[t] as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64);
                let need = self.army_need_with(win, troops[t], defense, 30);
                let send = (need * self.breakout_pct / 100).max(need).min(avail);
                if send < need {
                    continue;
                }
                // Cheapest relative to the stack sending it, enemy land first.
                let key = (owner[t].is_some() as u64, u64::MAX - need * 1000 / avail.max(1));
                if best.is_none_or(|b| (key.0, key.1) > (b.0, b.1)) {
                    best = Some((key.0, key.1, s, t, send));
                }
            }
        }
        best.map(|(_, _, s, t, send)| Action::Move { from: m.hexes[s], to: m.hexes[t], troops: send as u32 })
    }

    /// Mean + `sure_x10`/10 sd of attackers lost taking `def` troops behind `defense` milli.
    fn army_need_with(&self, win: u64, def: u32, defense: u64, sure_x10: u64) -> u64 {
        let d = defense.max(1);
        let per_win = (1_000_000 + d / 2) / d;
        let wins = (def as u64 * 1000).div_ceil(per_win);
        let q = 1000 - win;
        let mean = wins * q * 1000 / win;
        let var = wins * q * 1_000_000_000 / (win * win);
        let sd = (var as f64).sqrt() as u64;
        (mean + sd * sure_x10 / 10).div_ceil(1000) + 1
    }

    fn army_need(&self, win: u64, def: u32, defense: u64) -> u64 {
        self.army_need_with(win, def, defense, self.army.sure_x10)
    }

    /// Hexes of rival pockets that would surrender to us if we took `to`.
    fn pockets(m: &Map, owner: &[Option<PlayerId>], me: PlayerId, to: usize) -> u64 {
        const LIMIT: usize = 80;
        let mut gain = 0u64;
        let mut seen: Vec<usize> = vec![];
        for &n in &m.nbrs[to] {
            let Some(o) = owner[n].filter(|&o| o != me) else { continue };
            if seen.contains(&n) {
                continue;
            }
            let mut comp = vec![n];
            let mut k = 0;
            let mut enclosed = true;
            while k < comp.len() && enclosed {
                let c = comp[k];
                k += 1;
                if m.open_edge[c] {
                    enclosed = false;
                    break;
                }
                for &j in &m.nbrs[c] {
                    if j == to || owner[j] == Some(me) {
                        continue;
                    }
                    if owner[j] != Some(o) {
                        enclosed = false;
                        break;
                    }
                    if !comp.contains(&j) {
                        comp.push(j);
                    }
                }
                if comp.len() > LIMIT {
                    enclosed = false;
                }
            }
            seen.extend(&comp);
            if enclosed {
                gain += comp.len() as u64;
            }
        }
        gain
    }

    fn army_action(&mut self, view: &View, m: &Map) -> Option<Action> {
        let me = view.you;
        let alive = view.public_players.iter().filter(|p| p.alive).count() as u32;
        if alive > self.army.alive {
            self.state.rally = None;
            return None;
        }
        let slots = m.passable.len();
        let mut owner: Vec<Option<PlayerId>> = vec![None; slots];
        let mut troops = vec![0u32; slots];
        let mut wall = vec![1000u32; slots];
        let mut seen = vec![false; slots];
        let mut total = 0u64;
        let mut army: Option<usize> = None;
        for c in &view.visible_hexes {
            let Some(i) = m.slot(c.hex) else { continue };
            owner[i] = c.owner;
            troops[i] = c.troops;
            wall[i] = c.wall_milli;
            seen[i] = true;
            if c.owner == Some(me) {
                total += c.troops as u64;
                if army.is_none_or(|a| c.troops > troops[a]) {
                    army = Some(i);
                }
            }
        }
        let a = army?;
        let ours = |i: usize| owner[i] == Some(me);
        let enemy = |i: usize| owner[i].is_some_and(|o| o != me);
        // A mobilization in progress keeps gathering until the army is big enough.
        if let Some((t, left)) = self.state.rally {
            let done = left == 0 || !ours(t) || troops[t] as u64 * 100 >= total * self.army.ready_pct;
            if !done {
                self.state.rally = Some((t, left - 1));
                return Some(Action::Gather { target: m.hexes[t] });
            }
            self.state.rally = None;
        }
        let small = (troops[a] as u64) * 100 < total * self.army.regather_pct;
        let may_rally = self.state.last_rally.is_none_or(|t| view.tick >= t + self.army.regather_gap);
        if small && may_rally {
            // Rally onto the border hex facing enemy land nearest our troop-weighted centre.
            let (mut sq, mut sr, mut n) = (0i64, 0i64, 0i64);
            for i in 0..slots {
                if ours(i) {
                    let w = troops[i] as i64;
                    sq += m.hexes[i].q as i64 * w;
                    sr += m.hexes[i].r as i64 * w;
                    n += w;
                }
            }
            let centre = Hex::new((sq / n.max(1)) as i32, (sr / n.max(1)) as i32);
            let pad = (0..slots)
                .filter(|&i| ours(i) && m.nbrs[i].iter().any(|&j| enemy(j)))
                .min_by_key(|&i| (m.hexes[i].distance(centre), std::cmp::Reverse(troops[i]), i));
            if let Some(p) = pad {
                self.state.last_rally = Some(view.tick);
                self.state.rally = Some((p, self.army.gathers.saturating_sub(1)));
                return Some(Action::Gather { target: m.hexes[p] });
            }
        }
        if small {
            return None;
        }
        let win = self.rules.attacker_win_milli(view.tick) as u64;
        let have = troops[a] as u64;
        let keep = (have / self.army.keep_div).clamp(self.army.keep_min, self.army.keep_max);
        if have <= keep + 1 {
            return None;
        }
        let send = have - keep;
        let mut best: Option<(i64, usize)> = None;
        for &e in &m.nbrs[a] {
            if ours(e) {
                continue;
            }
            let hug = m.nbrs[e].iter().filter(|&&j| ours(j)).count() as i64;
            // A stack that wanders off on its own can be cut off and surrender whole, so it
            // only steps where it stays joined to our land or can't be enclosed.
            if hug < self.army.hug_min as i64 && !m.open_edge[e] {
                continue;
            }
            let def = troops[e];
            let h = self.rules.height_multiplier_milli(m.height[a], m.height[e]) as u64;
            let defense = (h * wall[e] as u64 / 1000).min(self.rules.maximum_total_defense_milli as u64);
            if self.army_need(win, def, defense) > send {
                continue;
            }
            let deeper = m.nbrs[e].iter().filter(|&&j| enemy(j)).count() as i64;
            let pocket = Self::pockets(m, &owner, me, e) as i64;
            let score = if enemy(e) { 1000 } else { 300 }
                + pocket * self.army.pocket
                + deeper * 40
                + hug * 30
                + (def as i64).min(100_000) / 1000;
            if best.is_none_or(|b| (score, std::cmp::Reverse(e)) > (b.0, std::cmp::Reverse(b.1))) {
                best = Some((score, e));
            }
        }
        if let Some((_, e)) = best {
            return Some(Action::Move { from: m.hexes[a], to: m.hexes[e], troops: send as u32 });
        }
        // Nothing to take next to the army: walk it through our land toward the nearest
        // rival hex.
        let mut prev = vec![usize::MAX; slots];
        prev[a] = a;
        let mut queue = VecDeque::from([a]);
        while let Some(c) = queue.pop_front() {
            if c != a && enemy(c) {
                let mut step = c;
                while prev[step] != a {
                    step = prev[step];
                }
                if !ours(step) || step == c {
                    return None;
                }
                return Some(Action::Move { from: m.hexes[a], to: m.hexes[step], troops: (have - 1) as u32 });
            }
            if c != a && !ours(c) {
                continue;
            }
            for &n in &m.nbrs[c] {
                if prev[n] == usize::MAX && (ours(n) || (seen[n] && enemy(n))) {
                    prev[n] = c;
                    queue.push_back(n);
                }
            }
        }
        None
    }
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clk: i32, ts: *mut Timespec) -> i32;
}

/// This thread's CPU time, for profiling without counting time spent descheduled.
fn cpu_now() -> std::time::Duration {
    let mut t = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(3, &mut t) };
    std::time::Duration::new(t.sec as u64, t.nsec as u32)
}
