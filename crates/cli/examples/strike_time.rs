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

extern "C" {
    fn clock_gettime(clk: i32, ts: *mut Timespec) -> i32;
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
        let mut st = State::new(settings(8, seed), named.iter().map(|s| s.name.clone()).collect()).unwrap();
        let mut bots: Vec<_> = named.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
        let (mut g_worst, mut g_tick) = (0u64, 0u32);
        while !st.is_over() {
            let cmds: TickCommands = (0..8 as PlayerId)
                .filter(|&p| !st.eliminated[p as usize])
                .map(|p| {
                    let view = st.view(p);
                    let (c0, w0) = (thread_cpu_ns(), Instant::now());
                    let cmd = bots[p as usize].act(&view);
                    let (c, w) = (thread_cpu_ns() - c0, w0.elapsed().as_nanos() as u64);
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
}
