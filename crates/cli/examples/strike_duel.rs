//! Mirrored 1v1 duels of a candidate against a base: each seed is played twice with the
//! seats swapped. Reports the candidate's win rate (draws count half) with its standard
//! error over pairs, how many games ended by conquest, and when.
//!
//! HEXARENA_ARENA=data/arena cargo run --release -p hexarena-cli --example strike_duel -- CAND BASE PAIRS [SEED]
//! `STRIKE_MAP` as in strike_eval.

use hexarena_arena::{entrants::load_all, play_on};
use hexarena_protocol::*;
use hexarena_sdk::{style, BotParams, Style};

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

fn strike_map(seed: u64) -> Option<MapPreset> {
    let choice = std::env::var("STRIKE_MAP").ok()?;
    if choice == "any" {
        let all = hexarena_engine::atlas::PRESETS;
        return Some(all[(seed.rotate_right(17) % all.len() as u64) as usize]);
    }
    Some(serde_json::from_value(serde_json::Value::String(choice.clone())).unwrap_or_else(|_| panic!("unknown map {choice}")))
}

/// The candidate's score in one duel (1 win, 0.5 draw, 0 loss), the final tick and
/// whether it ended by conquest.
fn duel(seed: u64, cand: &Style, base: &Style, cand_first: bool) -> (f64, u32, bool) {
    let named = |s: &Style, i: usize| Style { name: format!("Player {}", i + 1), ..s.clone() };
    let styles =
        if cand_first { vec![named(cand, 0), named(base, 1)] } else { vec![named(base, 0), named(cand, 1)] };
    let played = play_on(seed, &styles, u32::MAX, strike_map(seed)).expect("play");
    let result = played.result();
    let me: PlayerId = if cand_first { 0 } else { 1 };
    let score = match result.winner() {
        Some(w) if w == me => 1.0,
        Some(_) => 0.0,
        None => 0.5,
    };
    let conquest = result.final_tick < RulesConfig::default().max_ticks;
    (score, result.final_tick, conquest)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let cand = seat(&a[0]);
    let base = seat(&a[1]);
    let pairs: u64 = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(8);
    let seed0: u64 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(900_000);
    let mut per_pair = vec![];
    let (mut conquests, mut cand_conquests, mut games, mut ticks) = (0u32, 0u32, 0u32, 0u64);
    for k in 0..pairs {
        let seed = seed0 + k;
        let mut pair = 0.0;
        for first in [true, false] {
            let (s, t, c) = duel(seed, &cand, &base, first);
            games += 1;
            ticks += t as u64;
            conquests += c as u32;
            cand_conquests += (c && s == 1.0) as u32;
            pair += s / 2.0;
            eprintln!("seed {seed} cand {} score {s} tick {t}{}", if first { "first" } else { "second" }, if c { " conquest" } else { "" });
        }
        per_pair.push(pair);
    }
    let n = per_pair.len() as f64;
    let mean = per_pair.iter().sum::<f64>() / n;
    let var = per_pair.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1.0).max(1.0);
    println!(
        "candidate win rate {:.3} +/- {:.3} over {games} duels; conquests {conquests} ({cand_conquests} by the candidate); mean final tick {:.0}",
        mean,
        (var / n).sqrt(),
        ticks as f64 / games.max(1) as f64
    );
}
