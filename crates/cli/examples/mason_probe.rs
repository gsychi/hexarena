//! Mirrored-pair lobbies like `hexarena eval`, with per-bot v0.7 stats: region hexes
//! won and lost per game, demolitions, eliminations, closes, move times, and a check of
//! Mason's ring prediction against the engine's region captures.
//!
//! cargo run --release --example mason_probe -- --cand mason.json --base pangolin-v9 \
//!     --pairs 4 [--seed 900000] [--arena /workspace/data/arena] [--pool roster|none]

use hexarena_arena::eval::avp8;
use hexarena_arena::{entrants, seat_names, settings};
use hexarena_engine::State;
use hexarena_protocol::*;
use hexarena_sdk::brains::mason::rings::{Board, Loot, Tarjan};
use hexarena_sdk::{brains, roster, style, BotParams, Style};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
struct Tally {
    seats: u32,
    score: f64,
    avp: f64,
    region_won: u64,
    region_lost: u64,
    demolished: u64,
    eliminated: u64,
    closes: u64,
    moves: u64,
    total: Duration,
    slowest: Duration,
    wall_slowest: Duration,
    slowest_tick: u32,
    predicted: u64,
    predicted_ok: u64,
    /// Buildings that changed owner away from this seat: [Farm, Village, Quarry, Wall] by
    /// direct capture, then by a sealed region.
    lost_cap: [u64; 4],
    lost_seal: [u64; 4],
    /// Buildings this seat took from rivals by direct capture.
    won_cap: [u64; 4],
    /// Land held at tick 1500 / 3000 / 6000 (summed over seats alive then).
    land: [u64; 3],
}

/// Real-world map every game is played on (`--map europe`, ...); procedural if unset.
static MAP: OnceLock<Option<MapPreset>> = OnceLock::new();

fn bslot(b: Building) -> usize {
    match b {
        Building::Farm => 0,
        Building::Village => 1,
        Building::Quarry => 2,
        Building::Wall => 3,
    }
}

/// Layout audit as in `layout_yield`, per seat id and sample tick.
const SAMPLES: [u32; 4] = [1500, 3000, 6000, 9000];

#[derive(Default)]
struct Layout {
    seats: u64,
    owned: u64,
    income: u64,
    built: u64,
    built_income: u64,
    counts: [u64; 3],
    village_farms: [u64; 7],
    farm_villages: [u64; 7],
    /// Villages touching no Farm, by their best neighbour: an empty own farmable hex, an
    /// own farmable hex with another building, a farmable hex we don't own, no farmable hex.
    no_farm: [u64; 4],
}

thread_local! {
    static LAYOUT: std::cell::RefCell<BTreeMap<(String, u32), Layout>> = Default::default();
}

fn sample_layout(st: &State, seats: &[Style]) {
    LAYOUT.with(|acc| {
        let mut acc = acc.borrow_mut();
        for (p, s) in seats.iter().enumerate() {
            let p = p as PlayerId;
            if st.eliminated[p as usize] {
                continue;
            }
            let a = acc.entry((s.id.clone(), st.tick)).or_default();
            a.seats += 1;
            for h in st.board.hexes() {
                let Some(c) = st.cell(h) else { continue };
                if c.owner != Some(p) {
                    continue;
                }
                let inc = st.income(h) as u64;
                a.owned += 1;
                a.income += inc;
                let own = |b: Building| {
                    h.neighbors()
                        .iter()
                        .filter(|&&n| st.cell(n).is_some_and(|c| c.owner == Some(p) && c.building == Some(b)))
                        .count()
                };
                match c.building {
                    Some(Building::Farm) => {
                        a.counts[0] += 1;
                        a.farm_villages[own(Building::Village)] += 1;
                    }
                    Some(Building::Village) => {
                        a.counts[1] += 1;
                        a.village_farms[own(Building::Farm)] += 1;
                        if own(Building::Farm) == 0 {
                            // Why no Farm: the best a neighbour offers, in this order.
                            let rules = &st.settings.rules;
                            let why = h
                                .neighbors()
                                .iter()
                                .filter_map(|&n| st.cell(n).map(|c| (n, c)))
                                .map(|(n, c)| {
                                    let farmable = rules.terrain_allowed(Building::Farm, st.height(n));
                                    match (farmable, c.owner == Some(p), c.building) {
                                        (true, true, None) => 0,
                                        (true, true, Some(_)) => 1,
                                        (true, false, _) => 2,
                                        (false, _, _) => 3,
                                    }
                                })
                                .min()
                                .unwrap_or(3);
                            a.no_farm[why] += 1;
                        }
                    }
                    Some(Building::Quarry) => a.counts[2] += 1,
                    _ => {}
                }
                if matches!(c.building, Some(Building::Farm | Building::Village | Building::Quarry)) {
                    a.built += 1;
                    a.built_income += inc;
                }
            }
        }
    });
}

fn print_layout() {
    LAYOUT.with(|acc| {
        let acc = acc.borrow();
        if acc.is_empty() {
            return;
        }
        println!(
            "{:<14} {:>5} {:>5} {:>6} {:>6} {:>6} {:>5} {:>11}  {:<21}  {:<21}  {}",
            "layout", "tick", "seats", "land", "inc/hx", "inc/bh", "bh%", "F:V:Q", "farms/village 0..6", "villages/farm 0..6",
            "no-farm V%: empty/built/unowned/hill"
        );
        let pct = |h: &[u64; 7]| {
            let t: u64 = h.iter().sum::<u64>().max(1);
            h.iter().map(|v| format!("{:>3}", v * 100 / t)).collect::<Vec<_>>().join("")
        };
        for ((id, tick), a) in acc.iter() {
            let v = a.counts[1].max(1) as f64;
            println!(
                "{:<14} {:>5} {:>5} {:>6.0} {:>6.2} {:>6.2} {:>4.0}% {:>4.2}:1:{:>4.2}  {:<21}  {:<21}  {}",
                id,
                tick,
                a.seats,
                a.owned as f64 / a.seats.max(1) as f64,
                a.income as f64 / a.owned.max(1) as f64,
                a.built_income as f64 / a.built.max(1) as f64,
                a.built as f64 * 100.0 / a.owned.max(1) as f64,
                a.counts[0] as f64 / v,
                a.counts[2] as f64 / v,
                pct(&a.village_farms),
                pct(&a.farm_villages),
                a.no_farm.iter().map(|&x| format!("{:.0}", x as f64 * 100.0 / v)).collect::<Vec<_>>().join("/")
            );
        }
    });
}

fn seat_style(id: &str, arena: &str) -> Style {
    if let Some(s) = style(id) {
        return s;
    }
    if let Some(e) = entrants::load_all(std::path::Path::new(arena)).into_iter().find(|e| e.id == id) {
        return e.style();
    }
    let params: BotParams = serde_json::from_slice(&std::fs::read(id).expect("genome file")).expect("genome json");
    let name = std::path::Path::new(id).file_stem().unwrap().to_string_lossy().to_string();
    Style { id: name.clone(), name, blurb: String::new(), params, program: None }
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clock: i32, tp: *mut Timespec) -> i32;
}

/// CPU time this thread has used: immune to the preemption an overloaded machine adds
/// to wall-clock timings.
fn thread_cpu() -> Duration {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(3, &mut ts) };
    Duration::new(ts.sec as u64, ts.nsec as u32)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn play(seed: u64, seats: &[Style], tallies: &mut BTreeMap<String, Tally>, log: &mut Vec<Vec<f64>>) -> f64 {
    let styles: Vec<Style> = seats
        .iter()
        .enumerate()
        .map(|(i, s)| Style { name: format!("Player {}", i + 1), ..s.clone() })
        .collect();
    let n = styles.len();
    let mut s = settings(n as u16, seed);
    s.rules.map = MAP.get().copied().flatten();
    let mut st = State::new(s, seat_names(&styles)).unwrap();
    let mut bots: Vec<_> = styles.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
    let mut t: Vec<Tally> = vec![Tally::default(); n];
    log.clear();
    log.resize(n, vec![]);
    let check: Vec<bool> = seats.iter().map(|s| s.params.brain == "mason" && s.program.is_none()).collect();
    let mut board: Option<Board> = None;
    let mut tarjan = Tarjan::default();
    let mut out: Vec<Loot> = vec![];
    while !st.is_over() {
        let first = st.initiative_order().first().copied();
        let mut cmds = vec![];
        let mut predictions: Vec<(PlayerId, Hex, u32)> = vec![];
        for p in 0..n as PlayerId {
            if st.eliminated[p as usize] {
                continue;
            }
            let view = st.view(p);
            let started = Instant::now();
            let cpu = thread_cpu();
            let cmd = bots[p as usize].act(&view);
            let took_cpu = thread_cpu() - cpu;
            let took = started.elapsed();
            log[p as usize].push(took_cpu.as_secs_f64() * 1000.0);
            let tp = &mut t[p as usize];
            tp.moves += 1;
            tp.total += took_cpu;
            if took_cpu > tp.slowest {
                tp.slowest = took_cpu;
                tp.slowest_tick = st.tick;
            }
            tp.wall_slowest = tp.wall_slowest.max(took);
            if check[p as usize] && first == Some(p) {
                if let Action::Move { to, .. } = cmd.action {
                    let b = board.get_or_insert_with(|| Board::new(&view));
                    let w = 2 * view.radius + 1;
                    let mut mine = vec![false; b.slots];
                    for c in &view.visible_hexes {
                        if c.owner == Some(p) {
                            mine[((c.hex.r + view.radius) * w + c.hex.q + view.radius) as usize] = true;
                        }
                    }
                    let ti = ((to.r + view.radius) * w + to.q + view.radius) as usize;
                    if !mine[ti] {
                        let weights: Vec<Loot> = (0..b.slots)
                            .map(|i| Loot { nodes: 1, hexes: b.passable[i] as u32, ..Loot::default() })
                            .collect();
                        tarjan.run(b, &mine, &weights, &mut out);
                        predictions.push((p, to, out[ti].hexes));
                    }
                }
            }
            cmds.push((p, cmd));
        }
        st.step(&cmds);
        if SAMPLES.contains(&st.tick) {
            sample_layout(&st, seats);
        }
        if let Some(k) = [1500, 3000, 6000].iter().position(|&s| s == st.tick) {
            for h in st.board.hexes() {
                if let Some(p) = st.cell(h).and_then(|c| c.owner) {
                    t[p as usize].land[k] += 1;
                }
            }
        }
        let mut region_now: HashMap<PlayerId, u32> = HashMap::new();
        let mut captured_at: HashMap<PlayerId, Vec<Hex>> = HashMap::new();
        let move_to: HashMap<PlayerId, Hex> = cmds
            .iter()
            .filter_map(|(p, c)| match c.action {
                Action::Move { to, .. } => Some((*p, to)),
                _ => None,
            })
            .collect();
        for e in &st.last_events {
            match e {
                Event::Surrendered { capturer, from, hexes } => {
                    t[*capturer as usize].region_won += hexes.len() as u64;
                    t[*from as usize].region_lost += hexes.len() as u64;
                    *region_now.entry(*capturer).or_default() += hexes.len() as u32;
                    for &h in hexes {
                        if let Some(b) = st.cell(h).and_then(|c| c.building) {
                            t[*from as usize].lost_seal[bslot(b)] += 1;
                        }
                    }
                }
                Event::Captured { player, at, previous_owner: None, .. } => {
                    if move_to.get(player) != Some(at) {
                        t[*player as usize].region_won += 1;
                        *region_now.entry(*player).or_default() += 1;
                    }
                    captured_at.entry(*player).or_default().push(*at);
                }
                Event::Captured { player, at, previous_owner: Some(prev), building, .. } => {
                    if let Some(b) = building {
                        t[*prev as usize].lost_cap[bslot(*b)] += 1;
                        t[*player as usize].won_cap[bslot(*b)] += 1;
                    }
                    captured_at.entry(*player).or_default().push(*at)
                }
                Event::Demolished { player, building, .. } => {
                    t[*player as usize].demolished += 1;
                    if std::env::var("PROBE_DEMO").is_ok() {
                        eprintln!("demolished t{} p{} {:?}", st.tick, player, building);
                    }
                }
                Event::Eliminated { player } => t[*player as usize].eliminated += 1,
                _ => {}
            }
        }
        for (p, to, predicted) in predictions {
            let took = captured_at.get(&p).is_some_and(|v| v.contains(&to));
            if !took {
                continue;
            }
            let actual = region_now.get(&p).copied().unwrap_or(0);
            if predicted > 0 || actual > 0 {
                t[p as usize].predicted += 1;
                if predicted == actual {
                    t[p as usize].predicted_ok += 1;
                }
            }
        }
    }
    let result = st.result.clone().unwrap_or_else(|| st.final_result());
    let cutoff = RulesConfig::default().max_ticks;
    let closer = (result.final_tick < cutoff).then(|| result.winner()).flatten();
    for pl in &result.placements {
        let i = pl.player as usize;
        let a = avp8(pl.place_start, pl.place_end, n);
        let mut s = a;
        if closer == Some(pl.player) {
            t[i].closes += 1;
            let early = (cutoff - result.final_tick) as f64 / cutoff as f64;
            s -= 0.5 + 1.0 * early;
        }
        t[i].avp = a;
        t[i].score = s;
    }
    let mut cand_score = (0.0, 0);
    let mut base_score = (0.0, 0);
    for i in 0..n {
        let tp = &t[i];
        let e = tallies.entry(seats[i].id.clone()).or_default();
        e.seats += 1;
        e.score += tp.score;
        e.avp += tp.avp;
        e.region_won += tp.region_won;
        e.region_lost += tp.region_lost;
        for b in 0..4 {
            e.lost_cap[b] += tp.lost_cap[b];
            e.lost_seal[b] += tp.lost_seal[b];
            e.won_cap[b] += tp.won_cap[b];
        }
        for k in 0..3 {
            e.land[k] += tp.land[k];
        }
        e.demolished += tp.demolished;
        e.eliminated += tp.eliminated;
        e.closes += tp.closes;
        e.moves += tp.moves;
        e.total += tp.total;
        e.slowest = e.slowest.max(tp.slowest);
        e.wall_slowest = e.wall_slowest.max(tp.wall_slowest);
        e.predicted += tp.predicted;
        e.predicted_ok += tp.predicted_ok;
        if seats[i].id == "cand" {
            cand_score.0 += tp.score;
            cand_score.1 += 1;
        } else if seats[i].id == "base" {
            base_score.0 += tp.score;
            base_score.1 += 1;
        }
    }
    eprintln!(
        "seed {seed} tick {} closer {:?}: {}",
        result.final_tick,
        closer.map(|p| seats[p as usize].id.clone()),
        (0..n).map(|i| format!("{}={:.0}/w{}l{}/d{}/{:.1}ms@{}", seats[i].id, t[i].avp, t[i].region_won, t[i].region_lost, t[i].demolished, t[i].slowest.as_secs_f64() * 1000.0, t[i].slowest_tick)).collect::<Vec<_>>().join(" ")
    );
    if n == 2 {
        // A duel scores +1 for a candidate win, -1 for a loss.
        let avp = |id: &str| (0..n).find(|&i| seats[i].id == id).map_or(0.0, |i| t[i].avp);
        return (avp("base") - avp("cand")) / 7.0;
    }
    base_score.0 / base_score.1.max(1) as f64 - cand_score.0 / cand_score.1.max(1) as f64
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut f: HashMap<String, String> = HashMap::new();
    for w in args.chunks(2) {
        f.insert(w[0].trim_start_matches("--").to_string(), w.get(1).cloned().unwrap_or_default());
    }
    let arena = f.get("arena").cloned().unwrap_or_else(|| "/workspace/data/arena".into());
    // Entrants play from source unless --frozen 1 spawns their published binaries.
    let frozen = f.get("frozen").is_some_and(|v| v == "1");
    let load = |id: &str| {
        let s = seat_style(id, &arena);
        if frozen { s } else { Style { program: None, ..s } }
    };
    let cand = Style { id: "cand".into(), ..load(&f["cand"]) };
    let base = Style { id: "base".into(), ..load(&f["base"]) };
    let duel = f.get("duel").is_some_and(|v| v == "1");
    MAP.set(f.get("map").map(|m| serde_json::from_str(&format!("\"{m}\"")).expect("unknown --map"))).unwrap();
    let pairs: u64 = f.get("pairs").map_or(4, |v| v.parse().unwrap());
    let seed0: u64 = f.get("seed").map_or(900_000, |v| v.parse().unwrap());
    let pool: Vec<Style> = match f.get("pool").map(|s| s.as_str()) {
        Some("none") => vec![],
        _ => roster(),
    };
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut gains = vec![];
    for k in 0..pairs {
        let seed = seed0 + k;
        let mut rng = Rng(seed ^ 0x9E37_79B9_7F4A_7C15);
        let mut seats = if duel { vec![cand.clone()] } else { vec![cand.clone(); 3] };
        seats.extend(vec![base.clone(); if duel { 1 } else { 3 }]);
        for _ in 0..if duel { 0 } else { 2 } {
            seats.push(if pool.is_empty() { base.clone() } else { pool[rng.below(pool.len())].clone() });
        }
        for i in (1..seats.len()).rev() {
            let j = rng.below(i + 1);
            seats.swap(i, j);
        }
        let mirrored: Vec<Style> = seats
            .iter()
            .map(|s| match s.id.as_str() {
                "cand" => base.clone(),
                "base" => cand.clone(),
                _ => s.clone(),
            })
            .collect();
        if let Some(r) = f.get("repeat").map(|v| v.parse::<usize>().unwrap()) {
            // Same deterministic lobby r times: per tick the fastest run, so preemption
            // and steal time on a loaded machine drop out; then the slowest such tick.
            let mut best: Vec<Vec<f64>> = vec![];
            let mut log = vec![];
            for _ in 0..r {
                play(seed, &seats, &mut BTreeMap::new(), &mut log);
                if best.is_empty() {
                    best = log.clone();
                } else {
                    for (b, l) in best.iter_mut().zip(&log) {
                        for (x, y) in b.iter_mut().zip(l) {
                            *x = x.min(*y);
                        }
                    }
                }
            }
            for (i, b) in best.iter().enumerate() {
                let (mut mx, mut at) = (0.0, 0);
                for (k, &x) in b.iter().enumerate() {
                    if x > mx {
                        mx = x;
                        at = k;
                    }
                }
                let mean = b.iter().sum::<f64>() / b.len().max(1) as f64;
                let mut sorted = b.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let p999 = sorted.get(sorted.len() * 999 / 1000).copied().unwrap_or(0.0);
                println!("timing seed {seed} seat {i} {:<6} moves {:>5} mean {:.3} ms  p99.9 {:.3} ms  max {:.3} ms at move {}", seats[i].id, b.len(), mean, p999, mx, at);
            }
            continue;
        }
        let mut log = vec![];
        let g1 = play(seed, &seats, &mut tallies, &mut log);
        if f.contains_key("single") {
            gains.push(g1);
            break;
        }
        let g2 = play(seed, &mirrored, &mut tallies, &mut log);
        gains.push((g1 + g2) / 2.0);
        let m = gains.iter().sum::<f64>() / gains.len() as f64;
        let se = if gains.len() > 1 {
            (gains.iter().map(|g| (g - m).powi(2)).sum::<f64>() / (gains.len() - 1) as f64 / gains.len() as f64).sqrt()
        } else {
            f64::NAN
        };
        eprintln!("after {} pairs: gain {:+.2} ± {:.2}", gains.len(), m, se);
    }
    let m = gains.iter().sum::<f64>() / gains.len().max(1) as f64;
    let se = if gains.len() > 1 {
        (gains.iter().map(|g| (g - m).powi(2)).sum::<f64>() / (gains.len() - 1) as f64 / gains.len() as f64).sqrt()
    } else {
        f64::NAN
    };
    println!("gain {:+.3} ± {:.3} over {} pairs ({} games)", m, se, gains.len(), gains.len() * 2);
    println!("{:<14} {:>5} {:>6} {:>6} {:>7} {:>7} {:>5} {:>5} {:>5} {:>8} {:>8} {:>9} {:>8}", "bot", "seats", "score", "avp", "rgn+/g", "rgn-/g", "demo", "elim", "close", "cpu ms/mv", "cpu max", "ring ok", "wall max");
    for (id, e) in &tallies {
        let s = e.seats.max(1) as f64;
        println!(
            "{:<14} {:>5} {:>6.2} {:>6.2} {:>7.1} {:>7.1} {:>5} {:>5} {:>5} {:>8.3} {:>8.2} {:>4}/{:<4} {:>8.1}",
            id,
            e.seats,
            e.score / s,
            e.avp / s,
            e.region_won as f64 / s,
            e.region_lost as f64 / s,
            e.demolished,
            e.eliminated,
            e.closes,
            e.total.as_secs_f64() * 1000.0 / e.moves.max(1) as f64,
            e.slowest.as_secs_f64() * 1000.0,
            e.predicted_ok,
            e.predicted,
            e.wall_slowest.as_secs_f64() * 1000.0
        );
    }
    println!("buildings per seat-game (F/V/Q/W): lost by capture | lost by seal | won by capture; land held per seat at t1500/3000/6000");
    for (id, e) in &tallies {
        let s = e.seats.max(1) as f64;
        let f = |a: &[u64; 4]| a.iter().map(|&x| format!("{:.1}", x as f64 / s)).collect::<Vec<_>>().join("/");
        let land = e.land.iter().map(|&x| format!("{:.0}", x as f64 / s)).collect::<Vec<_>>().join("/");
        println!("{:<14} lost {:<22} seal {:<18} won {:<22} land {}", id, f(&e.lost_cap), f(&e.lost_seal), f(&e.won_cap), land);
    }
    print_layout();
}
