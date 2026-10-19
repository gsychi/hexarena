//! Siege: a bounded rescue-and-encircle decision loop for region-capture rules.
//!
//! Every tick it first protects threatened articulation necks, then compares every
//! affordable border capture with one- or two-capture enclosure opportunities. Captures
//! are rejected when they leave a one-hex salient under immediate counterattack. Idle
//! ticks reinforce a useful front or build a Village on deep interior land.

use crate::{
    Action, Bot, Building, Hex, ManagementCommand, PlayerCommand, PlayerId, RulesConfig, Style,
    View, VisibleHex, MILLI,
};
use std::cmp::Reverse;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

const REGION_LIMIT: usize = 96;
const RESCUE_SCAN_LIMIT: usize = 128;
const MOVE_BUDGET: Duration = Duration::from_micros(3_500);

struct Geometry {
    radius: i32,
    width: i32,
    valid: Vec<bool>,
    passable: Vec<bool>,
    height: Vec<i8>,
    hexes: Vec<Hex>,
    all_nbrs: Vec<Vec<usize>>,
    move_nbrs: Vec<Vec<usize>>,
    edge: Vec<bool>,
}

impl Geometry {
    fn new(view: &View) -> Self {
        let radius = view.radius;
        let width = 2 * radius + 1;
        let slots = (width * width) as usize;
        let mut valid = vec![false; slots];
        let mut passable = vec![false; slots];
        let mut height = vec![0; slots];
        let hexes: Vec<Hex> = (0..slots)
            .map(|i| Hex::new(i as i32 % width - radius, i as i32 / width - radius))
            .collect();
        for i in 0..slots {
            if let Some(t) = view.public_terrain[i] {
                valid[i] = true;
                passable[i] = t.passable();
                height[i] = t.height;
            }
        }
        let mut out = Self {
            radius,
            width,
            valid,
            passable,
            height,
            hexes,
            all_nbrs: vec![vec![]; slots],
            move_nbrs: vec![vec![]; slots],
            edge: vec![false; slots],
        };
        for i in 0..slots {
            if !out.valid[i] {
                continue;
            }
            for n in out.hexes[i].neighbors() {
                if let Some(j) = out.slot(n) {
                    out.all_nbrs[i].push(j);
                    if out.passable[j] {
                        out.move_nbrs[i].push(j);
                    }
                } else {
                    out.edge[i] = true;
                }
            }
        }
        out
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
    building: Option<Building>,
    income: u32,
}

#[derive(Clone)]
struct Choice {
    score: i64,
    action: Action,
}

#[derive(Clone, Copy, Default)]
struct Enclosure {
    land: usize,
    enemy: usize,
    income: u64,
    buildings: usize,
}

struct Knobs {
    sure_x10: u64,
    land: i64,
    enemy: i64,
    income: i64,
    building: i64,
    cost: i64,
    height: i64,
    fog: i64,
    safety: i64,
    enclosure: i64,
    enclosure_enemy: i64,
    lookahead: u32,
    two_discount: i64,
    thin_min_friends: usize,
    counter_margin: u64,
    source_keep: u64,
    rescue_min_land: usize,
    rescue_min_troops: u64,
    rescue_enemy_pct: u64,
    rescue_cap: usize,
    focus: i64,
    focus_weakest: bool,
    city_from: u32,
    city_until: u32,
    city_depth: u32,
    city_score: i64,
    city_keep: u32,
    gather_score: i64,
    demolish: bool,
    overtime_aggression: i64,
    sprint: u32,
}

impl Knobs {
    fn read(style: &Style) -> Self {
        let k = |name: &str, default: i64| style.params.knob(name, default).max(0);
        Self {
            sure_x10: k("sure_x10", 30) as u64,
            land: k("land", 120),
            enemy: k("enemy", 55),
            income: k("income", 35),
            building: k("building", 90),
            cost: k("cost", 8),
            height: k("height", 16),
            fog: k("fog", 3),
            safety: k("safety", 90),
            enclosure: k("enclosure", 150),
            enclosure_enemy: k("enclosure_enemy", 80),
            lookahead: k("lookahead", 2) as u32,
            two_discount: k("two_discount", 65),
            thin_min_friends: k("thin_min_friends", 2) as usize,
            counter_margin: k("counter_margin", 450) as u64,
            source_keep: k("source_keep", 15) as u64,
            rescue_min_land: k("rescue_min_land", 3) as usize,
            rescue_min_troops: k("rescue_min_troops", 120) as u64,
            rescue_enemy_pct: k("rescue_enemy_pct", 130) as u64,
            rescue_cap: k("rescue_cap", 60) as usize,
            focus: k("focus", 35),
            focus_weakest: k("focus_weakest", 1) > 0,
            city_from: k("city_from", 350) as u32,
            city_until: k("city_until", 7000) as u32,
            city_depth: k("city_depth", 2) as u32,
            city_score: k("city_score", 105),
            city_keep: k("city_keep", 25) as u32,
            gather_score: k("gather_score", 35),
            demolish: k("demolish", 1) > 0,
            overtime_aggression: k("overtime_aggression", 35),
            sprint: k("sprint", 500) as u32,
        }
    }
}

pub struct Siege {
    name: String,
    rules: RulesConfig,
    win: u32,
    k: Knobs,
    geo: Option<Geometry>,
    seen: Vec<Option<Cell>>,
}

impl Siege {
    pub fn new(style: Style) -> Self {
        Self {
            name: style.id.clone(),
            rules: RulesConfig::default(),
            win: RulesConfig::default().attacker_exchange_win_milli,
            k: Knobs::read(&style),
            geo: None,
            seen: vec![],
        }
    }

    fn owned(mine: &[bool], i: usize, added: &[usize]) -> bool {
        mine[i] || added.contains(&i)
    }

    fn fight_stats(&self, defenders: u32, defense: u32) -> (u64, u64) {
        let defense = defense.max(1) as u64;
        let kills_per_win = (1_000_000 + defense / 2) / defense;
        let wins = (defenders as u64 * 1000).div_ceil(kills_per_win);
        let p = self.win as u64;
        let q = 1000 - p;
        let mean = wins * q * 1000 / p;
        let variance_x1e6 = wins * q * 1_000_000_000 / (p * p);
        (mean, isqrt(variance_x1e6))
    }

    fn defense(&self, geo: &Geometry, from: usize, to: usize, wall_milli: u32) -> u32 {
        let height = self
            .rules
            .height_multiplier_milli(geo.height[from], geo.height[to]);
        (height as u64 * wall_milli as u64 / 1000)
            .min(self.rules.maximum_total_defense_milli as u64) as u32
    }

    fn attack_need(&self, geo: &Geometry, from: usize, to: usize, cell: Cell) -> (u64, u64) {
        let (mean, sd) =
            self.fight_stats(cell.troops, self.defense(geo, from, to, cell.wall_milli));
        let need = (mean + sd * self.k.sure_x10 / 10).div_ceil(1000) + 1;
        (need, mean.div_ceil(1000))
    }

    fn enemy_kills(&self, geo: &Geometry, from: usize, to: usize, troops: u32) -> u64 {
        let defense = self.defense(geo, from, to, MILLI) as u64;
        troops.saturating_sub(1) as u64 * self.win as u64 * 1000
            / ((1000 - self.win as u64) * defense.max(1))
    }

    /// Exact v0.7 topology with a strict work cap. Impassable cells remain in the region.
    fn enclosure(
        &self,
        geo: &Geometry,
        mine: &[bool],
        me: PlayerId,
        added: &[usize],
        trigger: usize,
    ) -> Enclosure {
        let mut done = vec![false; geo.valid.len()];
        for &i in added {
            done[i] = true;
        }
        let mut total = Enclosure::default();
        for &start in &geo.all_nbrs[trigger] {
            if done[start] || Self::owned(mine, start, added) {
                continue;
            }
            let mut region = vec![start];
            done[start] = true;
            let mut cursor = 0;
            let mut enclosed = true;
            while cursor < region.len() {
                let at = region[cursor];
                cursor += 1;
                if geo.edge[at] {
                    enclosed = false;
                }
                for &n in &geo.all_nbrs[at] {
                    if !done[n] && !Self::owned(mine, n, added) {
                        done[n] = true;
                        region.push(n);
                        if region.len() > REGION_LIMIT {
                            enclosed = false;
                            break;
                        }
                    }
                }
                if region.len() > REGION_LIMIT {
                    break;
                }
            }
            if !enclosed {
                continue;
            }
            for i in region {
                if !geo.passable[i] {
                    continue;
                }
                total.land += 1;
                if let Some(c) = self.seen[i] {
                    if c.owner.is_some_and(|o| o != me) {
                        total.enemy += 1;
                    }
                    total.income += c.income as u64;
                    total.buildings += usize::from(c.building.is_some());
                }
            }
        }
        total
    }

    fn enclosure_score(&self, e: Enclosure) -> i64 {
        e.land as i64 * self.k.enclosure
            + e.enemy as i64 * self.k.enclosure_enemy
            + e.income as i64 * self.k.income
            + e.buildings as i64 * self.k.building
    }

    fn weakest_visible(&self, view: &View) -> Option<PlayerId> {
        if !self.k.focus_weakest {
            return None;
        }
        let mut land: HashMap<PlayerId, usize> = HashMap::new();
        for c in self.seen.iter().flatten() {
            if let Some(owner) = c.owner.filter(|&o| o != view.you) {
                *land.entry(owner).or_default() += 1;
            }
        }
        view.public_players
            .iter()
            .filter(|p| p.alive && p.id != view.you)
            .filter_map(|p| land.get(&p.id).map(|&n| (n, p.id)))
            .min()
            .map(|x| x.1)
    }

    fn fog_gain(&self, geo: &Geometry, covered: &[bool], target: usize) -> usize {
        geo.valid
            .iter()
            .enumerate()
            .filter(|(i, valid)| {
                **valid && !covered[*i] && geo.hexes[*i].distance(geo.hexes[target]) <= 3
            })
            .count()
    }

    fn fog_covered(&self, geo: &Geometry, mine_list: &[usize]) -> Vec<bool> {
        let mut distance = vec![u8::MAX; geo.valid.len()];
        let mut queue = VecDeque::new();
        for &start in mine_list {
            distance[start] = 0;
            queue.push_back(start);
        }
        while let Some(at) = queue.pop_front() {
            if distance[at] == 3 {
                continue;
            }
            for &n in &geo.all_nbrs[at] {
                if distance[n] > distance[at] + 1 {
                    distance[n] = distance[at] + 1;
                    queue.push_back(n);
                }
            }
        }
        distance.into_iter().map(|d| d <= 3).collect()
    }

    fn counter_need(&self, geo: &Geometry, mine: &[bool], me: PlayerId, target: usize) -> u64 {
        geo.move_nbrs[target]
            .iter()
            .filter(|&&n| !mine[n])
            .filter_map(|&n| {
                let c = self.seen[n]?;
                (c.owner.is_some_and(|o| o != me))
                    .then(|| self.enemy_kills(geo, n, target, c.troops))
            })
            .max()
            .unwrap_or(0)
    }

    fn best_attack(
        &self,
        view: &View,
        geo: &Geometry,
        mine: &[bool],
        mine_list: &[usize],
        fog_covered: &[bool],
        started: Instant,
    ) -> Option<Choice> {
        let me = view.you;
        let focus = self.weakest_visible(view);
        let sprinting = view.tick + self.k.sprint >= self.rules.max_ticks;
        let overtime = view.tick.saturating_sub(self.rules.overtime_start) / 1000;
        let aggression = overtime as i64 * self.k.overtime_aggression;
        let mut best: Option<Choice> = None;
        for &from in mine_list {
            if started.elapsed() >= MOVE_BUDGET {
                break;
            }
            let Some(source) = self.seen[from] else {
                continue;
            };
            if source.troops < 2 {
                continue;
            }
            for &to in &geo.move_nbrs[from] {
                if mine[to] {
                    continue;
                }
                let Some(target) = self.seen[to] else {
                    continue;
                };
                let (need, expected_loss) = self.attack_need(geo, from, to, target);
                let available = source.troops as u64 - 1;
                if need > available {
                    continue;
                }
                let direct = self.enclosure(geo, mine, me, &[to], to);
                let friends = geo.move_nbrs[to].iter().filter(|&&n| mine[n]).count();
                let enemy_neighbors = geo.move_nbrs[to]
                    .iter()
                    .filter(|&&n| {
                        !mine[n] && self.seen[n].is_some_and(|c| c.owner.is_some_and(|o| o != me))
                    })
                    .count();
                let counter = self.counter_need(geo, mine, me, to);
                let hold = counter * self.k.counter_margin / 100;
                let send = if sprinting {
                    need
                } else {
                    need.max(expected_loss + hold).min(available)
                };
                let survivors = send.saturating_sub(expected_loss);
                let thin = friends < self.k.thin_min_friends && enemy_neighbors > 0;
                if !sprinting && direct.land == 0 && (thin || survivors < hold.min(40)) {
                    continue;
                }
                let source_left = source.troops as u64 - send;
                if !sprinting
                    && source_left < self.k.source_keep
                    && geo.move_nbrs[from]
                        .iter()
                        .any(|&n| self.seen[n].is_some_and(|c| c.owner.is_some_and(|o| o != me)))
                {
                    continue;
                }
                let mut two = Enclosure::default();
                if self.k.lookahead >= 2 && direct.land == 0 {
                    for &second in &geo.move_nbrs[to] {
                        if started.elapsed() >= MOVE_BUDGET {
                            break;
                        }
                        if mine[second] || second == from {
                            continue;
                        }
                        let Some(second_cell) = self.seen[second] else {
                            continue;
                        };
                        let (second_need, _) = self.attack_need(geo, to, second, second_cell);
                        if second_need + 1 > survivors {
                            continue;
                        }
                        let e = self.enclosure(geo, mine, me, &[to, second], second);
                        if self.enclosure_score(e) > self.enclosure_score(two) {
                            two = e;
                        }
                    }
                }
                let building = target.building.map_or(0, |b| match b {
                    Building::Village => self.k.building * 2,
                    Building::Farm | Building::Quarry => self.k.building,
                    Building::Wall => self.k.building / 2,
                });
                let safety_ratio = if hold == 0 {
                    1000
                } else {
                    (survivors * 1000 / hold).min(1000) as i64
                };
                let mut score = self.k.land
                    + target.income as i64 * self.k.income
                    + building
                    + geo.height[to] as i64 * self.k.height
                    + self.fog_gain(geo, fog_covered, to) as i64 * self.k.fog
                    + safety_ratio * self.k.safety / 1000
                    + self.enclosure_score(direct)
                    + self.enclosure_score(two) * self.k.two_discount / 100
                    - expected_loss as i64 * self.k.cost
                    + aggression;
                if target.owner.is_some() {
                    score += self.k.enemy;
                }
                if target.owner == focus {
                    score += self.k.focus;
                }
                let choice = Choice {
                    score,
                    action: Action::Move {
                        from: geo.hexes[from],
                        to: geo.hexes[to],
                        troops: send as u32,
                    },
                };
                if best.as_ref().is_none_or(|b| choice.score > b.score) {
                    best = Some(choice);
                }
            }
        }
        best
    }

    /// Finds a small or troop-rich component hanging behind an enemy-threatened own neck.
    fn rescue(
        &self,
        view: &View,
        geo: &Geometry,
        mine: &[bool],
        mine_list: &[usize],
        started: Instant,
    ) -> Option<Choice> {
        let me = view.you;
        let mut best: Option<(i64, usize, usize)> = None;
        for &neck in mine_list {
            if started.elapsed() >= MOVE_BUDGET {
                break;
            }
            let own_nbrs: Vec<usize> = geo.move_nbrs[neck]
                .iter()
                .copied()
                .filter(|&n| mine[n])
                .collect();
            if own_nbrs.len() < 2 {
                continue;
            }
            let Some(neck_cell) = self.seen[neck] else {
                continue;
            };
            let threat = geo.move_nbrs[neck]
                .iter()
                .filter_map(|&n| {
                    let c = self.seen[n]?;
                    if !c.owner.is_some_and(|o| o != me) {
                        return None;
                    }
                    let kills = self.enemy_kills(geo, n, neck, c.troops);
                    (kills * 100 >= neck_cell.troops as u64 * self.k.rescue_enemy_pct)
                        .then_some((kills, n))
                })
                .max_by_key(|x| x.0);
            let Some((danger, attacker)) = threat else {
                continue;
            };
            let mut visited = vec![false; mine.len()];
            visited[neck] = true;
            let mut components: Vec<(usize, u64, usize)> = vec![];
            for &start in &own_nbrs {
                if visited[start] {
                    continue;
                }
                let mut queue = VecDeque::from([start]);
                visited[start] = true;
                let mut size = 0;
                let mut troops = 0;
                let mut strongest = start;
                while let Some(at) = queue.pop_front() {
                    size += 1;
                    let t = self.seen[at].map_or(0, |c| c.troops) as u64;
                    troops += t;
                    if t > self.seen[strongest].map_or(0, |c| c.troops) as u64 {
                        strongest = at;
                    }
                    if size >= RESCUE_SCAN_LIMIT {
                        break;
                    }
                    for &n in &geo.move_nbrs[at] {
                        if mine[n] && !visited[n] && n != neck {
                            visited[n] = true;
                            queue.push_back(n);
                        }
                    }
                }
                components.push((size, troops, strongest));
            }
            if components.len() < 2 {
                continue;
            }
            components.sort_by_key(|c| c.0);
            let (size, troops, source) = components[0];
            if size > self.k.rescue_cap
                || (size < self.k.rescue_min_land && troops < self.k.rescue_min_troops)
            {
                continue;
            }
            let value = size as i64 * 1000 + troops.min(10_000) as i64 + danger as i64;
            if best.is_none_or(|b| value > b.0) {
                best = Some((value, neck, attacker));
            }
            let _ = source;
        }
        let (value, neck, attacker) = best?;

        // Capturing the stack that would close the ring is the cleanest rescue.
        let mut counter: Option<Choice> = None;
        let target = self.seen[attacker]?;
        for &from in &geo.move_nbrs[attacker] {
            if !mine[from] {
                continue;
            }
            let source = self.seen[from]?;
            let (need, _) = self.attack_need(geo, from, attacker, target);
            if need < source.troops as u64 {
                let c = Choice {
                    score: 1_000_000 + value,
                    action: Action::Move {
                        from: geo.hexes[from],
                        to: geo.hexes[attacker],
                        troops: need as u32,
                    },
                };
                if counter.as_ref().is_none_or(|b| c.score > b.score) {
                    counter = Some(c);
                }
            }
        }
        if counter.is_some() {
            return counter;
        }

        // Otherwise feed the neck from the richest adjacent safe hex.
        if let Some((&from, troops)) = geo.move_nbrs[neck]
            .iter()
            .filter(|&&n| mine[n])
            .filter_map(|n| self.seen[*n].map(|c| (n, c.troops)))
            .filter(|(_, t)| *t > 2)
            .max_by_key(|(_, t)| *t)
        {
            return Some(Choice {
                score: 900_000 + value,
                action: Action::Move {
                    from: geo.hexes[from],
                    to: geo.hexes[neck],
                    troops: troops - 1,
                },
            });
        }
        None
    }

    fn depths(&self, geo: &Geometry, mine: &[bool]) -> Vec<u32> {
        let mut depth = vec![u32::MAX; mine.len()];
        let mut queue = VecDeque::new();
        for i in 0..mine.len() {
            if mine[i] && geo.move_nbrs[i].iter().any(|&n| !mine[n]) {
                depth[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(at) = queue.pop_front() {
            for &n in &geo.move_nbrs[at] {
                if mine[n] && depth[n] == u32::MAX {
                    depth[n] = depth[at] + 1;
                    queue.push_back(n);
                }
            }
        }
        depth
    }

    fn city(
        &self,
        view: &View,
        geo: &Geometry,
        mine: &[bool],
        mine_list: &[usize],
    ) -> Option<Choice> {
        if view.tick < self.k.city_from || view.tick >= self.k.city_until {
            return None;
        }
        let depth = self.depths(geo, mine);
        let mut best: Option<(u32, u32, Reverse<usize>)> = None;
        for &i in mine_list {
            let Some(c) = self.seen[i] else { continue };
            if c.building.is_some() || depth[i] < self.k.city_depth {
                continue;
            }
            let near_quarry = geo.move_nbrs[i]
                .iter()
                .any(|&n| self.seen[n].is_some_and(|c| c.building == Some(Building::Quarry)));
            let cost = self.rules.build_cost(Building::Village, near_quarry).troops;
            if c.troops < cost + 1 + self.k.city_keep {
                continue;
            }
            let key = (depth[i], c.troops, Reverse(i));
            if best.is_none_or(|b| key > b) {
                best = Some(key);
            }
        }
        let (_, _, Reverse(at)) = best?;
        Some(Choice {
            score: self.k.city_score,
            action: Action::Build {
                at: geo.hexes[at],
                building: Building::Village,
            },
        })
    }

    fn gather(
        &self,
        view: &View,
        geo: &Geometry,
        mine: &[bool],
        mine_list: &[usize],
    ) -> Option<Choice> {
        let mut best: Option<(u32, Reverse<usize>)> = None;
        for &i in mine_list {
            let Some(c) = self.seen[i] else { continue };
            let targets = geo.move_nbrs[i]
                .iter()
                .filter(|&&n| !mine[n])
                .filter_map(|&n| self.seen[n].map(|x| x.troops))
                .min();
            if targets.is_some() {
                let key = (c.troops, Reverse(i));
                if best.is_none_or(|b| key > b) {
                    best = Some(key);
                }
            }
        }
        let (_, Reverse(at)) = best?;
        let contact = mine_list.iter().any(|&i| {
            geo.move_nbrs[i]
                .iter()
                .any(|&n| self.seen[n].is_some_and(|c| c.owner.is_some_and(|o| o != view.you)))
        });
        if contact {
            let mut distance = vec![u32::MAX; mine.len()];
            let mut toward = vec![usize::MAX; mine.len()];
            let mut queue = VecDeque::from([at]);
            distance[at] = 0;
            while let Some(current) = queue.pop_front() {
                for &n in &geo.move_nbrs[current] {
                    if mine[n] && distance[n] == u32::MAX {
                        distance[n] = distance[current] + 1;
                        toward[n] = current;
                        queue.push_back(n);
                    }
                }
            }
            let source = mine_list
                .iter()
                .copied()
                .filter(|&i| distance[i] > 0 && distance[i] < u32::MAX)
                .filter(|&i| {
                    !geo.move_nbrs[i].iter().any(|&n| {
                        self.seen[n].is_some_and(|c| c.owner.is_some_and(|o| o != view.you))
                    })
                })
                .filter_map(|i| self.seen[i].map(|c| (c.troops, Reverse(distance[i]), i)))
                .filter(|x| x.0 > self.k.source_keep as u32 + 1)
                .max();
            if let Some((troops, _, from)) = source {
                return Some(Choice {
                    score: self.k.gather_score,
                    action: Action::Move {
                        from: geo.hexes[from],
                        to: geo.hexes[toward[from]],
                        troops: troops - self.k.source_keep as u32,
                    },
                });
            }
            return None;
        }
        Some(Choice {
            score: self.k.gather_score,
            action: Action::Gather {
                target: geo.hexes[at],
            },
        })
    }

    fn doomed_building(
        &self,
        view: &View,
        geo: &Geometry,
        mine: &[bool],
        mine_list: &[usize],
    ) -> Option<Choice> {
        if !self.k.demolish {
            return None;
        }
        let mut best: Option<(i64, usize)> = None;
        for &i in mine_list {
            let Some(c) = self.seen[i] else { continue };
            let Some(building) = c.building else { continue };
            let danger = geo.move_nbrs[i]
                .iter()
                .filter(|&&n| !mine[n])
                .filter_map(|&n| {
                    let e = self.seen[n]?;
                    e.owner
                        .is_some_and(|o| o != view.you)
                        .then(|| self.enemy_kills(geo, n, i, e.troops))
                })
                .max()
                .unwrap_or(0);
            if danger <= c.troops as u64 * 2 {
                continue;
            }
            let value = match building {
                Building::Village => 300,
                Building::Farm | Building::Quarry => 180,
                Building::Wall => 80,
            };
            if best.is_none_or(|b| value > b.0) {
                best = Some((value, i));
            }
        }
        let (score, at) = best?;
        Some(Choice {
            score,
            action: Action::Demolish { at: geo.hexes[at] },
        })
    }

    fn management(&self, view: &View) -> Vec<ManagementCommand> {
        let villages: Vec<&VisibleHex> = view
            .visible_hexes
            .iter()
            .filter(|c| {
                c.owner == Some(view.you)
                    && c.building
                        .as_ref()
                        .is_some_and(|b| b.kind == Building::Village && b.links.is_empty())
            })
            .collect();
        for farm in view.visible_hexes.iter().filter(|c| {
            c.owner == Some(view.you)
                && c.building.as_ref().is_some_and(|b| {
                    b.kind == Building::Farm
                        && b.link_cooldown == 0
                        && b.links.len() < self.rules.farm.sockets as usize
                })
        }) {
            if let Some(village) = villages.iter().find(|v| v.hex.distance(farm.hex) == 1) {
                return vec![ManagementCommand::SetFarmLink {
                    farm: farm.hex,
                    village: village.hex,
                    enabled: true,
                }];
            }
        }
        vec![]
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

impl Bot for Siege {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        let started = Instant::now();
        if let Some(rules) = &view.rules {
            self.rules = rules.clone();
        }
        self.win = self.rules.attacker_win_milli(view.tick);
        let geo = self.geo.take().unwrap_or_else(|| Geometry::new(view));
        if self.seen.is_empty() {
            self.seen = vec![None; geo.valid.len()];
        }
        let mut mine = vec![false; geo.valid.len()];
        let mut mine_list = vec![];
        for c in &view.visible_hexes {
            let Some(i) = geo.slot(c.hex) else { continue };
            self.seen[i] = Some(Cell {
                owner: c.owner,
                troops: c.troops,
                wall_milli: c.wall_milli,
                building: c.building.as_ref().map(|b| b.kind),
                income: c.income,
            });
            if c.owner == Some(view.you) {
                mine[i] = true;
                mine_list.push(i);
            }
        }

        let fog_covered = self.fog_covered(&geo, &mine_list);
        let rescue = self.rescue(view, &geo, &mine, &mine_list, started);
        let attack = self.best_attack(view, &geo, &mine, &mine_list, &fog_covered, started);
        let city = self.city(view, &geo, &mine, &mine_list);
        let demolish = self.doomed_building(view, &geo, &mine, &mine_list);
        let gather = self.gather(view, &geo, &mine, &mine_list);
        let action = rescue
            .or_else(|| {
                [attack, city, demolish, gather]
                    .into_iter()
                    .flatten()
                    .max_by_key(|c| c.score)
            })
            .map_or(Action::NoOp, |c| c.action);
        let management = self.management(view);
        self.geo = Some(geo);
        PlayerCommand { management, action }
    }
}
