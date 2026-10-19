//! Small local imitation of the league gate: mirrored 8-seat pairs, 3 challenger seats,
//! 3 champion seats and 2 sparring seats, plus per-seat build counts and land.
//! usage: cargo run --release --example bunny_gate -- --me cand.json [--champ g.json]
//!        [--pairs 4] [--seed 900] [--threads 2] [--spar relay,predator]

use hexarena_arena::play;
use hexarena_league::eval::avp8;
use hexarena_sdk::{style, BotParams, Style};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

fn flags() -> HashMap<String, String> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut m = HashMap::new();
    let mut i = 0;
    while i + 1 < a.len() {
        if let Some(k) = a[i].strip_prefix("--") {
            m.insert(k.to_string(), a[i + 1].clone());
            i += 1;
        }
        i += 1;
    }
    m
}

fn genome(path: &str, id: &str) -> Style {
    if let Some(s) = style(path) {
        return s;
    }
    if !path.ends_with(".json") {
        let l = hexarena_league::League::load(std::path::Path::new("/workspace/data/league")).expect("league");
        let g = l.generations.iter().find(|g| g.id == path).expect("generation");
        return Style { id: id.into(), name: id.into(), blurb: String::new(), params: g.params.clone(), program: None };
    }
    let p: BotParams = serde_json::from_slice(&std::fs::read(path).expect("genome file")).expect("genome json");
    Style { id: id.into(), name: id.into(), blurb: String::new(), params: p, program: None }
}

fn champion() -> Style {
    let l = hexarena_league::League::load(std::path::Path::new("/workspace/data/league")).expect("league");
    let g = l.champion();
    Style { id: "champ".into(), name: "champ".into(), blurb: String::new(), params: g.params.clone(), program: None }
}

struct Game {
    tick: u32,
    me: Vec<(f64, u32, u32)>,
    ch: Vec<(f64, u32, u32)>,
    closer: Option<String>,
}

fn main() {
    let f = flags();
    let me = genome(f.get("me").expect("--me"), "me");
    let ch = f.get("champ").map(|p| genome(p, "champ")).unwrap_or_else(champion);
    let pairs: u64 = f.get("pairs").and_then(|v| v.parse().ok()).unwrap_or(4);
    let seed0: u64 = f.get("seed").and_then(|v| v.parse().ok()).unwrap_or(900);
    let threads: usize = f.get("threads").and_then(|v| v.parse().ok()).unwrap_or(2);
    let spar: Vec<Style> = f
        .get("spar")
        .map(String::as_str)
        .unwrap_or("g8,g10,g11,ridgeline")
        .split(',')
        .map(|s| genome(s, s))
        .collect();
    let mut lobbies: Vec<(u64, Vec<(String, Style)>)> = vec![];
    for k in 0..pairs {
        let seed = seed0 + k;
        let mut seats: Vec<(String, Style)> = vec![];
        for _ in 0..3 {
            seats.push(("me".into(), me.clone()));
            seats.push(("ch".into(), ch.clone()));
        }
        seats.push(("sp".into(), spar[(k as usize * 2) % spar.len()].clone()));
        seats.push(("sp".into(), spar[(k as usize * 2 + 1) % spar.len()].clone()));
        let rot = (seed as usize * 5) % 8;
        seats.rotate_left(rot);
        let mirrored: Vec<(String, Style)> = seats
            .iter()
            .map(|(t, s)| match t.as_str() {
                "me" => ("ch".into(), ch.clone()),
                "ch" => ("me".into(), me.clone()),
                _ => (t.clone(), s.clone()),
            })
            .collect();
        lobbies.push((seed, seats));
        lobbies.push((seed, mirrored));
    }
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Game>>> = Mutex::new((0..lobbies.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((seed, seats)) = lobbies.get(i) else { break };
                let styles: Vec<Style> = seats
                    .iter()
                    .enumerate()
                    .map(|(j, (_, s))| Style { name: format!("Player {}", j + 1), ..s.clone() })
                    .collect();
                let p = play(*seed, &styles, u32::MAX).expect("play");
                let r = p.result();
                let mut builds = vec![0u32; 8];
                for rec in &p.state.records {
                    builds[rec.builder as usize] += 1;
                }
                let mut g = Game { tick: r.final_tick, me: vec![], ch: vec![], closer: None };
                for pl in &r.placements {
                    let seat = pl.player as usize;
                    let v = (avp8(pl.place_start, pl.place_end, 8), builds[seat], pl.land);
                    match seats[seat].0.as_str() {
                        "me" => g.me.push(v),
                        "ch" => g.ch.push(v),
                        _ => {}
                    }
                }
                if r.final_tick < 3000 {
                    g.closer = r.winner().map(|w| seats[w as usize].0.clone());
                }
                eprintln!(
                    "game {i} seed {seed} tick {} me {:?} ch {:?} closer {:?}",
                    g.tick, g.me, g.ch, g.closer
                );
                out.lock().unwrap()[i] = Some(g);
            });
        }
    });
    let games: Vec<Game> = out.into_inner().unwrap().into_iter().map(|g| g.unwrap()).collect();
    let score = |g: &Game, who: &str| {
        let v = if who == "me" { &g.me } else { &g.ch };
        let mut m = v.iter().map(|x| x.0).sum::<f64>() / v.len() as f64;
        if g.closer.as_deref() == Some(who) {
            m -= 0.5 + (3000.0 - g.tick as f64) / 3000.0;
        }
        m
    };
    let gains: Vec<f64> = games
        .chunks(2)
        .map(|p| (score(&p[0], "ch") + score(&p[1], "ch")) / 2.0 - (score(&p[0], "me") + score(&p[1], "me")) / 2.0)
        .collect();
    let n = gains.len() as f64;
    let m = gains.iter().sum::<f64>() / n;
    let se = (gains.iter().map(|g| (g - m).powi(2)).sum::<f64>() / (n - 1.0).max(1.0) / n).sqrt();
    let avg = |who: &str, k: usize| {
        let v: Vec<f64> = games
            .iter()
            .flat_map(|g| if who == "me" { g.me.clone() } else { g.ch.clone() })
            .map(|x| [x.0, x.1 as f64, x.2 as f64][k])
            .collect();
        v.iter().sum::<f64>() / v.len() as f64
    };
    println!(
        "gain {m:.3} ± {se:.3} over {} pairs | me avp {:.2} builds {:.1} land {:.0} | champ avp {:.2} builds {:.1} land {:.0} | closes me {} ch {}",
        gains.len(),
        avg("me", 0),
        avg("me", 1),
        avg("me", 2),
        avg("ch", 0),
        avg("ch", 1),
        avg("ch", 2),
        games.iter().filter(|g| g.closer.as_deref() == Some("me")).count(),
        games.iter().filter(|g| g.closer.as_deref() == Some("ch")).count(),
    );
}
