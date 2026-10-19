//! Procedural asymmetric map generation with topology validation.

use crate::atlas;
use crate::rng::{fbm, splitmix, Rng};
use crate::Board;
use hexarena_protocol::{Building, Hex, MatchSettings, Terrain, TerrainKind};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct GeneratedMap {
    pub radius: i32,
    pub terrain: Vec<Option<Terrain>>,
    pub garrison: Vec<u32>,
    pub buildings: Vec<Option<Building>>,
    pub spawns: Vec<Hex>,
}

const HEIGHT_QUANTILES_PCT: [usize; 5] = [15, 20, 30, 20, 15];

pub(crate) fn planar(h: Hex) -> (f64, f64) {
    (h.q as f64 + h.r as f64 * 0.5, h.r as f64 * 0.866_025_4)
}

fn kind_for(height: i8) -> TerrainKind {
    if height >= 0 {
        TerrainKind::Mountain
    } else {
        TerrainKind::Water
    }
}

fn gen_terrain(board: &Board, seed: u64, s: &MatchSettings) -> Option<Vec<Option<Terrain>>> {
    let hexes = board.hexes();
    let n = hexes.len();
    let scale = 3.0 + board.radius as f64 * 0.35;
    let hv: Vec<f64> = hexes
        .iter()
        .map(|&h| {
            let (x, y) = planar(h);
            fbm(seed ^ 0xA1, x, y, scale * 1.4, 4)
        })
        .collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| hv[a].partial_cmp(&hv[b]).unwrap());
    let mut height = vec![0i8; n];
    let mut start = 0;
    for (band, pct) in HEIGHT_QUANTILES_PCT.iter().enumerate() {
        let end = if band == 4 { n } else { start + n * pct / 100 };
        for &i in &order[start..end] {
            height[i] = band as i8 - 2;
        }
        start = end;
    }
    let mv: Vec<f64> = hexes
        .iter()
        .enumerate()
        .map(|(i, &h)| {
            let (x, y) = planar(h);
            let ridge = (fbm(seed ^ 0xB2, x, y, scale, 4) - 0.5).abs();
            0.65 * fbm(seed ^ 0xC3, x, y, scale * 0.8, 4)
                + 0.35 * (1.0 - ridge * 2.0)
                + 0.12 * (height[i].abs() as f64 / 2.0)
        })
        .collect();
    order.sort_by(|&a, &b| mv[b].partial_cmp(&mv[a]).unwrap());
    let blocked_count = n * s.rules.impassable_target_pct as usize / 100;
    let mut blocked = vec![false; n];
    for &i in &order[..blocked_count] {
        blocked[i] = true;
    }
    let mut terrain: Vec<Option<Terrain>> = vec![None; board.slots()];
    for (i, &h) in hexes.iter().enumerate() {
        let kind = if blocked[i] {
            kind_for(height[i])
        } else {
            TerrainKind::Open
        };
        terrain[board.index(h).unwrap()] = Some(Terrain {
            height: height[i],
            kind,
        });
    }
    keep_largest_component(board, &mut terrain);
    let open = hexes
        .iter()
        .filter(|&&h| passable(board, &terrain, h))
        .count();
    let imp_pct = (n - open) * 100 / n;
    if imp_pct < s.rules.impassable_min_pct as usize
        || imp_pct > s.rules.impassable_max_pct as usize
    {
        return None;
    }
    Some(terrain)
}

pub fn passable(board: &Board, terrain: &[Option<Terrain>], h: Hex) -> bool {
    board
        .index(h)
        .and_then(|i| terrain[i])
        .map(|t| t.passable())
        .unwrap_or(false)
}

pub(crate) fn keep_largest_component(board: &Board, terrain: &mut [Option<Terrain>]) {
    let hexes = board.hexes();
    let mut comp = vec![usize::MAX; board.slots()];
    let mut sizes = vec![];
    for &h in &hexes {
        let i = board.index(h).unwrap();
        if comp[i] != usize::MAX || !passable(board, terrain, h) {
            continue;
        }
        let id = sizes.len();
        let mut size = 0;
        let mut q = VecDeque::from([h]);
        comp[i] = id;
        while let Some(c) = q.pop_front() {
            size += 1;
            for nb in c.neighbors() {
                if let Some(j) = board.index(nb) {
                    if comp[j] == usize::MAX && passable(board, terrain, nb) {
                        comp[j] = id;
                        q.push_back(nb);
                    }
                }
            }
        }
        sizes.push(size);
    }
    let best = (0..sizes.len()).max_by_key(|&i| sizes[i]).unwrap_or(0);
    for &h in &hexes {
        let i = board.index(h).unwrap();
        if comp[i] != usize::MAX && comp[i] != best {
            let t = terrain[i].as_mut().unwrap();
            t.kind = kind_for(t.height);
        }
    }
}

/// BFS from `spawn` to any open hex at distance >= `far`, skipping `blocked`. Returns the path.
fn route(
    board: &Board,
    terrain: &[Option<Terrain>],
    spawn: Hex,
    far: i32,
    blocked: &[Hex],
) -> Option<Vec<Hex>> {
    let mut parent = vec![None; board.slots()];
    let mut seen = vec![false; board.slots()];
    seen[board.index(spawn).unwrap()] = true;
    let mut q = VecDeque::from([spawn]);
    while let Some(c) = q.pop_front() {
        if c.distance(spawn) >= far {
            let mut path = vec![c];
            let mut cur = c;
            while let Some(p) = parent[board.index(cur).unwrap()] {
                path.push(p);
                cur = p;
            }
            return Some(path);
        }
        for nb in c.neighbors() {
            if let Some(j) = board.index(nb) {
                if !seen[j] && passable(board, terrain, nb) && !blocked.contains(&nb) {
                    seen[j] = true;
                    parent[j] = Some(c);
                    q.push_back(nb);
                }
            }
        }
    }
    None
}

/// Conservative check: two routes sharing no hex except the spawn.
pub fn has_two_independent_routes(board: &Board, terrain: &[Option<Terrain>], spawn: Hex) -> bool {
    let first = match route(board, terrain, spawn, 6, &[]) {
        Some(p) => p,
        None => return false,
    };
    let interior: Vec<Hex> = first.into_iter().filter(|&h| h != spawn).collect();
    route(board, terrain, spawn, 6, &interior).is_some()
}

pub(crate) fn open_within(board: &Board, terrain: &[Option<Terrain>], c: Hex, r: i32) -> usize {
    let mut n = 0;
    for dq in -r..=r {
        for dr in (-r).max(-dq - r)..=r.min(-dq + r) {
            if passable(board, terrain, Hex::new(c.q + dq, c.r + dr)) {
                n += 1;
            }
        }
    }
    n
}

/// Expected attacker cost to take a hex, as a percentage of troops (integer).
pub fn expected_capture_cost(
    rules: &hexarena_protocol::RulesConfig,
    garrison: u32,
    from_h: i8,
    to_h: i8,
) -> u32 {
    let d = rules.defense_milli(from_h, to_h, None) as u64;
    let p = rules.attacker_exchange_win_milli as u64;
    (garrison as u64 * d * (1000 - p) / p / 1000) as u32
}

/// Count adjacent or two-step expansion targets the starting army can plausibly afford.
pub fn affordable_expansions(
    board: &Board,
    terrain: &[Option<Terrain>],
    garrison: &[u32],
    spawn: Hex,
    rules: &hexarena_protocol::RulesConfig,
) -> usize {
    let height = |h: Hex| {
        board
            .index(h)
            .and_then(|i| terrain[i])
            .map(|t| t.height)
            .unwrap_or(0)
    };
    let budget = (rules.starting_troops.saturating_sub(1)) * 85 / 100;
    let mut seen = std::collections::HashSet::new();
    for n in spawn.neighbors() {
        if !passable(board, terrain, n) {
            continue;
        }
        let cost = expected_capture_cost(
            rules,
            garrison[board.index(n).unwrap()],
            height(spawn),
            height(n),
        );
        if cost <= budget {
            seen.insert(n);
        }
        for n2 in n.neighbors() {
            if n2 != spawn && passable(board, terrain, n2) && n2.distance(spawn) == 2 {
                let c2 = cost
                    + expected_capture_cost(
                        rules,
                        garrison[board.index(n2).unwrap()],
                        height(n),
                        height(n2),
                    );
                if c2 <= budget {
                    seen.insert(n2);
                }
            }
        }
    }
    seen.len()
}

pub fn generate(s: &MatchSettings) -> Result<GeneratedMap, String> {
    let rules = &s.rules;
    if s.players < rules.min_players || s.players > rules.max_players {
        return Err(format!(
            "player count {} outside {}..={}",
            s.players, rules.min_players, rules.max_players
        ));
    }
    if let Some(preset) = rules.map {
        return atlas::generate(preset, s);
    }
    let radius = rules.radius_for_players(s.players);
    let board = Board::new(radius);
    let hexes = board.hexes();
    for attempt in 0..300u64 {
        let tseed = splitmix(s.terrain_seed ^ splitmix(attempt));
        let Some(terrain) = gen_terrain(&board, tseed, s) else {
            continue;
        };
        let open: Vec<Hex> = hexes
            .iter()
            .copied()
            .filter(|&h| passable(&board, &terrain, h))
            .collect();
        let mut rng = Rng::new(s.spawn_seed ^ splitmix(attempt.wrapping_add(77)));
        let mut garrison = vec![0u32; board.slots()];
        let gscale = 4.0 + radius as f64 * 0.3;
        let mut grng = Rng::new(tseed ^ 0x6A09);
        for &h in &open {
            let (x, y) = planar(h);
            let t = fbm(tseed ^ 0xD4, x, y, gscale, 3);
            let base =
                rules.neutral_min as f64 + (rules.neutral_max - rules.neutral_min) as f64 * t;
            let jitter = grng.range(0, 4) as i32 - 2;
            garrison[board.index(h).unwrap()] = (base.round() as i32 + jitter)
                .clamp(rules.neutral_min as i32, rules.neutral_max as i32)
                as u32;
        }
        let min_dist = rules.spawn_min_distance.max(6);
        let mut cands = open.clone();
        rng.shuffle(&mut cands);
        let mut spawns: Vec<Hex> = vec![];
        for &c in &cands {
            if spawns.len() == s.players as usize {
                break;
            }
            if spawns.iter().any(|p| p.distance(c) < min_dist)
                || open_within(&board, &terrain, c, 4) < 14
            {
                continue;
            }
            if has_two_independent_routes(&board, &terrain, c)
                && affordable_expansions(&board, &terrain, &garrison, c, rules) >= 2
            {
                spawns.push(c);
            }
        }
        if spawns.len() != s.players as usize {
            continue;
        }
        for &sp in &spawns {
            garrison[board.index(sp).unwrap()] = 0;
        }
        let buildings = vec![None; board.slots()];
        rng.shuffle(&mut spawns);
        return Ok(GeneratedMap {
            radius,
            terrain,
            garrison,
            buildings,
            spawns,
        });
    }
    Err("map generation failed after 300 attempts".into())
}
