use hexarena_arena::play;
use hexarena_protocol::{Event, RulesConfig};
use hexarena_sdk::{style as roster_style, BotParams, Style};
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
struct Metrics {
    games: usize,
    surrendered_hexes_per_game: f64,
    conquest_rate: f64,
    mean_move_ms: f64,
    max_move_ms: f64,
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .ok_or("usage: siege_metrics PARAMS.json [GAMES] [MAX_TICKS]")?;
    let games = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let max_ticks = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(u32::MAX);
    let params: BotParams =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let style = Style {
        id: "siege-metrics".into(),
        name: "Siege metrics".into(),
        blurb: String::new(),
        params,
        program: None,
    };
    let mut surrendered = 0usize;
    let mut conquests = 0usize;
    let mut moves = 0u64;
    let mut elapsed = Duration::ZERO;
    let mut slowest = Duration::ZERO;
    for seed in 1..=games as u64 {
        let mut styles = vec![style.clone(); 4];
        for id in ["ridgeline", "expander", "raider", "warden"] {
            styles.push(roster_style(id).unwrap());
        }
        let played = play(seed, &styles, max_ticks)?;
        let result = played.result();
        let cutoff = max_ticks.min(RulesConfig::default().max_ticks);
        conquests += usize::from(result.final_tick < cutoff);
        for tick in &played.events {
            for event in tick {
                if let Event::Surrendered {
                    capturer, hexes, ..
                } = event
                {
                    if *capturer >= 4 {
                        continue;
                    }
                    surrendered += hexes.len();
                }
            }
        }
        for timing in played.move_time.into_iter().take(4) {
            moves += timing.moves;
            elapsed += timing.total;
            slowest = slowest.max(timing.slowest);
        }
    }
    let out = Metrics {
        games,
        surrendered_hexes_per_game: surrendered as f64 / games as f64 / 4.0,
        conquest_rate: conquests as f64 / games as f64,
        mean_move_ms: elapsed.as_secs_f64() * 1000.0 / moves.max(1) as f64,
        max_move_ms: slowest.as_secs_f64() * 1000.0,
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
    Ok(())
}
