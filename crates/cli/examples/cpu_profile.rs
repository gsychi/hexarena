use hexarena_arena::{seat_names, settings};
use hexarena_engine::State;
use hexarena_sdk::{brains, Bot, BotParams, PlayerCommand, PlayerId, Style};
use std::time::Duration;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock_id: i32, time: *mut Timespec) -> i32;
}

fn thread_cpu() -> Duration {
    let mut time = Timespec { sec: 0, nsec: 0 };
    unsafe {
        clock_gettime(3, &mut time);
    }
    Duration::new(time.sec as u64, time.nsec as u32)
}

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: cpu_profile PARAMS.json [seed]")?;
    let seed = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(667);
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let params: BotParams = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let style = Style {
        id: "profile".into(),
        name: "Profile".into(),
        blurb: String::new(),
        params,
        program: None,
    };
    let styles = vec![style; 8];
    let mut state = State::new(settings(8, seed), seat_names(&styles))?;
    let mut bots: Vec<Box<dyn Bot>> = styles
        .into_iter()
        .map(brains::build)
        .collect::<Result<_, _>>()?;
    let mut moves = 0u64;
    let mut total = Duration::ZERO;
    let mut slowest = Duration::ZERO;
    while !state.is_over() && state.tick < state.settings.rules.max_ticks {
        let mut commands: Vec<(PlayerId, PlayerCommand)> = Vec::new();
        for player in 0..bots.len() as PlayerId {
            if state.eliminated[player as usize] {
                continue;
            }
            let view = state.view(player);
            let start = thread_cpu();
            let command = bots[player as usize].act(&view);
            let elapsed = thread_cpu().saturating_sub(start);
            moves += 1;
            total += elapsed;
            slowest = slowest.max(elapsed);
            if elapsed >= Duration::from_millis(2) {
                eprintln!(
                    "slow tick={} player={} cpu_ms={:.6}",
                    state.tick,
                    player,
                    elapsed.as_secs_f64() * 1000.0
                );
            }
            commands.push((player, command));
        }
        state.step(&commands);
    }
    println!(
        "ticks={} moves={} mean_cpu_ms={:.6} slowest_cpu_ms={:.6}",
        state.tick,
        moves,
        total.as_secs_f64() * 1000.0 / moves as f64,
        slowest.as_secs_f64() * 1000.0
    );
    Ok(())
}
