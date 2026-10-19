//! In-process mirrored comparison with move timing, for brains compiled into this build.
//!
//! cargo run --release --example apex_lab -- cand.json base.json PAIRS [SEED] [POOL]
//! POOL is `roster` (default, like `hexarena eval`) or `base` (base fills the 2 extra seats).
//! Prints per-pair results, the gain in places (positive = candidate better) and the
//! candidate's mean / max decision time.

use hexarena_arena::eval::{avp8, LOBBY};
use hexarena_arena::{seat_names, settings};
use hexarena_engine::State;
use hexarena_protocol::{MatchResult, PlayerId, TickCommands};
use hexarena_sdk::{brains, Bot};
use hexarena_protocol::RulesConfig;
use hexarena_sdk::{roster, BotParams, Style};
use std::time::Duration;

fn load(path: &str) -> Style {
    if let Some(s) = hexarena_sdk::style(path) {
        return s;
    }
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).expect("read params")).expect("json");
    let inner = v.get("params").cloned().unwrap_or(v);
    let params: BotParams = serde_json::from_value(inner).expect("params");
    Style { id: path.into(), name: path.into(), blurb: String::new(), params, program: None }
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}
extern "C" {
    fn clock_gettime(clk: i32, ts: *mut Timespec) -> i32;
}
fn cpu_now() -> Duration {
    let mut t = Timespec { sec: 0, nsec: 0 };
    unsafe { clock_gettime(3, &mut t) };
    Duration::new(t.sec as u64, t.nsec as u32)
}

#[derive(Clone, Copy, Default)]
struct Timing {
    moves: u64,
    total: Duration,
    slowest: Duration,
    cpu_slowest: Duration,
    slow_tick: u32,
}

fn play(seed: u64, styles: &[Style]) -> (MatchResult, Vec<Timing>) {
    let mut st = State::new(settings(styles.len() as u16, seed), seat_names(styles)).unwrap();
    let mut bots: Vec<Box<dyn Bot>> = styles.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
    let mut tm = vec![Timing::default(); styles.len()];
    // With DUAL set, candidate seats run on two identical bots and each move is timed as
    // the faster of the two, which filters out time the machine spent elsewhere.
    let dual = std::env::var("DUAL").is_ok();
    let mut twins: Vec<Option<Box<dyn Bot>>> = styles
        .iter()
        .map(|s| (dual && s.id == "candidate").then(|| brains::build(s.clone()).unwrap()))
        .collect();
    let maxt: u32 = std::env::var("MAXT").ok().map_or(u32::MAX, |v| v.parse().unwrap());
    while !st.is_over() && st.tick < maxt {
        let cmds: TickCommands = (0..styles.len() as PlayerId)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| {
                let view = st.view(p);
                let w = std::time::Instant::now();
                let c0 = cpu_now();
                let cmd = bots[p as usize].act(&view);
                let mut cpu = cpu_now() - c0;
                if let Some(t) = twins[p as usize].as_mut() {
                    let c1 = cpu_now();
                    let again = t.act(&view);
                    cpu = cpu.min(cpu_now() - c1);
                    if again != cmd {
                        eprintln!("twin bots disagree at tick {} (a deadline cut one short)", st.tick);
                    }
                }
                let took = w.elapsed();
                let t = &mut tm[p as usize];
                t.moves += 1;
                t.total += took;
                t.slowest = t.slowest.max(took);
                if cpu > t.cpu_slowest {
                    t.cpu_slowest = cpu;
                    t.slow_tick = st.tick;
                }
                (p, cmd)
            })
            .collect();
        st.step(&cmds);
    }
    (st.result.clone().unwrap_or_else(|| st.final_result()), tm)
}

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i + 1));
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cand = Style { id: "candidate".into(), ..load(&a[1]) };
    let base = Style { id: "base".into(), ..load(&a[2]) };
    let pairs: usize = a.get(3).map_or(8, |s| s.parse().unwrap());
    let seed0: u64 = a.get(4).map_or(500_000, |s| s.parse().unwrap());
    let pool_base = a.get(5).is_some_and(|s| s == "base");
    let pool = roster();
    let cutoff = RulesConfig::default().max_ticks as f64;
    let mut gains = vec![];
    let (mut moves, mut total, mut slowest, mut cpu_slow) = (0u64, Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut bslow = Duration::ZERO;
    let mut closes = (0, 0);
    let started = std::time::Instant::now();
    for k in 0..pairs {
        let seed = seed0.wrapping_add(k as u64);
        let mut rng = Rng(seed ^ 0x5EA7);
        let mut seats = vec![cand.clone(); 3];
        seats.extend(vec![base.clone(); 3]);
        for _ in 0..LOBBY - 6 {
            seats.push(if pool_base { base.clone() } else { pool[rng.below(pool.len())].clone() });
        }
        rng.shuffle(&mut seats);
        let mirrored: Vec<Style> = seats
            .iter()
            .map(|s| match s.id.as_str() {
                "candidate" => base.clone(),
                "base" => cand.clone(),
                _ => s.clone(),
            })
            .collect();
        let mut score = [0.0f64; 2];
        let mut line = String::new();
        for lobby in [&seats, &mirrored] {
            let named: Vec<Style> =
                lobby.iter().enumerate().map(|(i, s)| Style { name: format!("Player {}", i + 1), ..s.clone() }).collect();
            let (r, mt) = play(seed, &named);
            let maxt: u32 = std::env::var("MAXT").ok().map_or(u32::MAX, |v| v.parse().unwrap());
            let closer = (r.final_tick < (cutoff as u32).min(maxt)).then(|| r.winner()).flatten().map(|p| p as usize);
            let mut sums = [(0.0, 0usize); 2];
            for pl in &r.placements {
                let i = pl.player as usize;
                let mut s = avp8(pl.place_start, pl.place_end, lobby.len());
                if closer == Some(i) {
                    s -= 0.5 + (cutoff - r.final_tick as f64).max(0.0) / cutoff;
                    if lobby[i].id == "candidate" {
                        closes.0 += 1
                    } else if lobby[i].id == "base" {
                        closes.1 += 1
                    }
                }
                let side = match lobby[i].id.as_str() {
                    "candidate" => 0,
                    "base" => 1,
                    _ => continue,
                };
                sums[side].0 += s;
                sums[side].1 += 1;
                if side == 0 {
                    let t = mt[i];
                    moves += t.moves;
                    total += t.total;
                    slowest = slowest.max(t.slowest);
                    if t.cpu_slowest > cpu_slow {
                        cpu_slow = t.cpu_slowest;
                        line += &format!(" [cpu max {:.2}ms @t{}]", t.cpu_slowest.as_secs_f64() * 1000.0, t.slow_tick);
                    }
                } else {
                    bslow = bslow.max(mt[i].cpu_slowest);
                }
            }
            score[0] += sums[0].0 / sums[0].1 as f64 / 2.0;
            score[1] += sums[1].0 / sums[1].1 as f64 / 2.0;
            line += &format!(" t{} c{:?}", r.final_tick, closer.map(|c| lobby[c].id.clone()));
        }
        let g = score[1] - score[0];
        gains.push(g);
        let n = gains.len() as f64;
        let m = gains.iter().sum::<f64>() / n;
        let se = if n > 1.0 { (gains.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0) / n).sqrt() } else { f64::NAN };
        println!(
            "pair {k} seed {seed}: cand {:.2} base {:.2} gain {g:+.2} | running {m:+.3} ± {se:.3} |{line} | {:.0}s",
            score[0],
            score[1],
            started.elapsed().as_secs_f64()
        );
    }
    let n = gains.len() as f64;
    let m = gains.iter().sum::<f64>() / n;
    let se = (gains.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0) / n).sqrt();
    println!(
        "RESULT gain {m:+.3} ± {se:.3} over {} games; closes cand {} base {}; cand mean {:.3} ms wall-max {:.3} ms cpu-max {:.3} ms over {moves} moves; base cpu-max {:.3} ms",
        2 * pairs,
        closes.0,
        closes.1,
        total.as_secs_f64() * 1000.0 / moves.max(1) as f64,
        slowest.as_secs_f64() * 1000.0,
        cpu_slow.as_secs_f64() * 1000.0,
        bslow.as_secs_f64() * 1000.0
    );
}
