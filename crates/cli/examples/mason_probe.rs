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
    let mut st = State::new(settings(n as u16, seed), seat_names(&styles)).unwrap();
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
                }
                Event::Captured { player, at, previous_owner: None, .. } => {
                    if move_to.get(player) != Some(at) {
                        t[*player as usize].region_won += 1;
                        *region_now.entry(*player).or_default() += 1;
                    }
                    captured_at.entry(*player).or_default().push(*at);
                }
                Event::Captured { player, at, .. } => captured_at.entry(*player).or_default().push(*at),
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
    base_score.0 / base_score.1.max(1) as f64 - cand_score.0 / cand_score.1.max(1) as f64
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut f: HashMap<String, String> = HashMap::new();
    for w in args.chunks(2) {
        f.insert(w[0].trim_start_matches("--").to_string(), w.get(1).cloned().unwrap_or_default());
    }
    let arena = f.get("arena").cloned().unwrap_or_else(|| "/workspace/data/arena".into());
    let cand = Style { id: "cand".into(), ..seat_style(&f["cand"], &arena) };
    let base = Style { id: "base".into(), ..seat_style(&f["base"], &arena) };
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
        let mut seats = vec![cand.clone(); 3];
        seats.extend(vec![base.clone(); 3]);
        for _ in 0..2 {
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
}
