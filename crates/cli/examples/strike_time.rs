//! Worst-case move time of one bot: plays lobbies with it in `SEATS` seats and the rest
//! filled from a base, timing every `act` of the timed seats both by wall clock and by
//! the thread's own CPU time (which a busy machine's preemption doesn't inflate).
//!
//! cargo run --release -p hexarena-cli --example strike_time -- BOT BASE GAMES [SEATS] [SEED]

use hexarena_arena::{entrants::load_all, settings};
use hexarena_engine::State;
use hexarena_protocol::*;
use hexarena_sdk::{brains, style, BotParams, Style};
use std::time::Instant;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

#[repr(C)]
struct Rusage {
    utime: [i64; 2],
    stime: [i64; 2],
    rest: [i64; 14],
}

extern "C" {
    fn clock_gettime(clk: i32, ts: *mut Timespec) -> i32;
    fn getrusage(who: i32, usage: *mut Rusage) -> i32;
}

/// (user, system) CPU microseconds of this thread so far.
fn thread_rusage_us() -> (u64, u64) {
    let mut r = Rusage { utime: [0; 2], stime: [0; 2], rest: [0; 14] };
    // RUSAGE_THREAD
    unsafe { getrusage(1, &mut r) };
    ((r.utime[0] * 1_000_000 + r.utime[1]) as u64, (r.stime[0] * 1_000_000 + r.stime[1]) as u64)
}

fn thread_cpu_ns() -> u64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    // CLOCK_THREAD_CPUTIME_ID
    unsafe { clock_gettime(3, &mut ts) };
    ts.sec as u64 * 1_000_000_000 + ts.nsec as u64
}

fn seat(id: &str) -> Style {
    if let Some(s) = style(id) {
        return s;
    }
    let dir = std::env::var("HEXARENA_ARENA").unwrap_or_else(|_| "data/arena".into());
    if let Some(e) = load_all(std::path::Path::new(&dir)).into_iter().find(|e| e.id == id) {
        return e.style();
    }
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(id).expect("read genome")).unwrap();
    let p: BotParams = serde_json::from_value(v.get("params").cloned().unwrap_or(v)).unwrap();
    Style { id: id.into(), name: id.into(), blurb: String::new(), params: p, program: None }
}

/// `STRIKE_MAP`: unset for procedural boards, `any` for a real-world map picked from the
/// seed as self-play does, or a map id such as `japan`.
fn strike_map(seed: u64) -> Option<MapPreset> {
    let choice = std::env::var("STRIKE_MAP").ok()?;
    if choice == "any" {
        let all = hexarena_engine::atlas::PRESETS;
        return Some(all[(seed.rotate_right(17) % all.len() as u64) as usize]);
    }
    Some(serde_json::from_value(serde_json::Value::String(choice.clone())).unwrap_or_else(|_| panic!("unknown map {choice}")))
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bot = seat(&a[0]);
    let base = seat(&a[1]);
    let games: u64 = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(2);
    let timed: usize = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(3);
    let seed0: u64 = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(700_000);
    let (mut worst_cpu, mut worst_wall, mut total_cpu, mut moves) = (0u64, 0u64, 0u64, 0u64);
    let mut over = 0u64;
    let mut hist = [0u64; 8];
    // A move over budget is replayed at once on the same view: if the replay is fast the
    // first reading was the machine, not the move. Replays cost the game nothing but can
    // nudge a bot's cadence state, so the game may drift after one.
    let (mut confirmed, mut worst_confirmed) = (0u64, 0u64);
    for g in 0..games {
        let seed = seed0 + g;
        let mut styles: Vec<Style> = (0..8).map(|i| if i < timed { bot.clone() } else { base.clone() }).collect();
        styles.rotate_left((g % 8) as usize);
        let is_timed: Vec<bool> = styles.iter().map(|s| s.id == bot.id).collect();
        let named: Vec<Style> = styles
            .iter()
            .enumerate()
            .map(|(i, s)| Style { name: format!("Player {}", i + 1), ..s.clone() })
            .collect();
        let mut s = settings(8, seed);
        s.rules.map = strike_map(seed);
        let mut st = State::new(s, named.iter().map(|s| s.name.clone()).collect()).unwrap();
        let mut bots: Vec<_> = named.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
        let (mut g_worst, mut g_tick) = (0u64, 0u32);
        while !st.is_over() {
            let cmds: TickCommands = (0..8 as PlayerId)
                .filter(|&p| !st.eliminated[p as usize])
                .map(|p| {
                    let view = st.view(p);
                    let (u0, s0) = thread_rusage_us();
                    let (c0, w0) = (thread_cpu_ns(), Instant::now());
                    let cmd = bots[p as usize].act(&view);
                    let (c, w) = (thread_cpu_ns() - c0, w0.elapsed().as_nanos() as u64);
                    let (u1, s1) = thread_rusage_us();
                    if c > 5_000_000 {
                        eprintln!("  slow move at tick {}: cpu {:.3} ms (user {:.3} ms, system {:.3} ms)", st.tick, c as f64 / 1e6, (u1 - u0) as f64 / 1e3, (s1 - s0) as f64 / 1e3);
                    }
                    if is_timed[p as usize] {
                        moves += 1;
                        total_cpu += c;
                        worst_wall = worst_wall.max(w);
                        if c > g_worst {
                            g_worst = c;
                            g_tick = st.tick;
                        }
                        if c > 5_000_000 {
                            over += 1;
                            let r0 = thread_cpu_ns();
                            let _ = bots[p as usize].act(&view);
                            let again = thread_cpu_ns() - r0;
                            eprintln!("    replayed: cpu {:.3} ms", again as f64 / 1e6);
                            worst_confirmed = worst_confirmed.max(again.min(c));
                            if again > 5_000_000 {
                                confirmed += 1;
                            }
                        } else {
                            worst_confirmed = worst_confirmed.max(c);
                        }
                        hist[((c / 500_000) as usize).min(7)] += 1;
                    }
                    (p, cmd)
                })
                .collect();
            st.step(&cmds);
        }
        worst_cpu = worst_cpu.max(g_worst);
        eprintln!("game {g} seed {seed}: final tick {} worst cpu {:.3} ms at tick {g_tick}", st.tick, g_worst as f64 / 1e6);
    }
    println!(
        "{} moves: worst thread-cpu {:.3} ms, worst wall {:.3} ms, mean cpu {:.4} ms, moves over 5 ms (cpu) {over}",
        moves,
        worst_cpu as f64 / 1e6,
        worst_wall as f64 / 1e6,
        total_cpu as f64 / 1e6 / moves.max(1) as f64
    );
    println!("cpu histogram (0.5 ms bins): {hist:?}");
    println!(
        "over 5 ms on the replay too: {confirmed}; worst move counting a fast replay as the move's cost: {:.3} ms",
        worst_confirmed as f64 / 1e6
    );
}
