//! Attack metrics for a candidate against a base, in the same mirrored 8-player lobbies
//! `hexarena eval` plays (3 seats each plus 2 roster CPUs, same seeds and shuffle).
//! Besides the eval score it counts, per seat: enemy captures (before tick 3000 too),
//! hexes lost, hexes won by encirclement, eliminations dealt (who took a dead seat's
//! last hex), deaths, enemy buildings captured, final land and closes; and the
//! candidate's slowest and mean move.
//!
//! cargo run --release -p hexarena-cli --example strike_eval -- CAND BASE PAIRS [SEED] [FIRST]
//! CAND/BASE: a roster CPU, an arena id (from $HEXARENA_ARENA or data/arena) or a genome
//! .json. FIRST skips the first pairs, so long runs can be split up. Uses
//! HEXARENA_EVAL_THREADS threads (default 1).

use hexarena_arena::eval::{avp8, LOBBY};
use hexarena_arena::{entrants::load_all, play_on};
use hexarena_protocol::*;
use hexarena_sdk::{roster, style, BotParams, Style};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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

#[derive(Default, Clone)]
struct Side {
    seats: f64,
    score: f64,
    avp: f64,
    closes: f64,
    enemy_caps: f64,
    early_caps: f64,
    neutral: f64,
    lost: f64,
    surrendered: f64,
    kills: f64,
    kills_strong: f64,
    deaths: f64,
    land: f64,
    bld_caps: f64,
    land3000: f64,
}

struct Game {
    ids: Vec<String>,
    stats: Vec<Side>,
    final_tick: u32,
    closer: Option<usize>,
    slowest_ms: f64,
    mean_ms: f64,
}

fn run(seed: u64, seats: &[Style]) -> Game {
    let styles: Vec<Style> = seats
        .iter()
        .enumerate()
        .map(|(i, s)| Style { name: format!("Player {}", i + 1), blurb: String::new(), ..s.clone() })
        .collect();
    let played = play_on(seed, &styles, u32::MAX, strike_map(seed)).expect("play");
    if let Ok(dir) = std::env::var("STRIKE_REPLAYS") {
        let tag: String = seats.iter().map(|s| if s.id == "candidate" { 'C' } else if s.id == "base" { 'B' } else { 'r' }).collect();
        let path = format!("{dir}/{seed}-{tag}.replay.json");
        std::fs::write(path, serde_json::to_vec(&played.replay()).unwrap()).unwrap();
    }
    let n = seats.len();
    let mut st = vec![Side { seats: 1.0, ..Default::default() }; n];
    let mut land = vec![1i64; n];
    let mut last_taker: HashMap<usize, usize> = HashMap::new();
    let roster_ids: Vec<String> = roster().into_iter().map(|s| s.id).collect();
    for (t, evs) in played.events.iter().enumerate() {
        for e in evs {
            match e {
                Event::Captured { player, previous_owner, building, .. } => {
                    let p = *player as usize;
                    land[p] += 1;
                    match previous_owner {
                        None => st[p].neutral += 1.0,
                        Some(o) => {
                            let o = *o as usize;
                            land[o] -= 1;
                            st[p].enemy_caps += 1.0;
                            st[o].lost += 1.0;
                            if t < 3000 {
                                st[p].early_caps += 1.0;
                            }
                            if building.is_some_and(|b| b == Building::Village || b == Building::Farm) {
                                st[p].bld_caps += 1.0;
                            }
                            last_taker.insert(o, p);
                        }
                    }
                }
                Event::Surrendered { capturer, from, hexes } => {
                    let (c, f) = (*capturer as usize, *from as usize);
                    land[c] += hexes.len() as i64;
                    land[f] -= hexes.len() as i64;
                    st[c].surrendered += hexes.len() as f64;
                    last_taker.insert(f, c);
                }
                Event::Eliminated { player } => {
                    let p = *player as usize;
                    st[p].deaths += 1.0;
                    if let Some(&k) = last_taker.get(&p) {
                        st[k].kills += 1.0;
                        if !roster_ids.contains(&seats[p].id) {
                            st[k].kills_strong += 1.0;
                        }
                    }
                }
                _ => {}
            }
        }
        if t == 2999 {
            for p in 0..n {
                st[p].land3000 = land[p].max(0) as f64;
            }
        }
    }
    let result = played.result();
    let cutoff = RulesConfig::default().max_ticks;
    let closer = (result.final_tick < cutoff).then(|| result.winner()).flatten().map(|p| p as usize);
    for pl in &result.placements {
        let p = pl.player as usize;
        st[p].avp = avp8(pl.place_start, pl.place_end, n);
        st[p].land = pl.land as f64;
        st[p].score = st[p].avp;
        if closer == Some(p) {
            st[p].closes = 1.0;
            let early = (cutoff as f64 - result.final_tick as f64).max(0.0) / cutoff as f64;
            st[p].score -= 0.5 + early;
        }
    }
    let mut slowest = 0.0f64;
    let (mut total, mut moves) = (0.0f64, 0u64);
    for (i, s) in seats.iter().enumerate() {
        if s.id == "candidate" {
            let mt = &played.move_time[i];
            slowest = slowest.max(mt.slowest.as_secs_f64() * 1000.0);
            total += mt.total.as_secs_f64() * 1000.0;
            moves += mt.moves;
        }
    }
    Game {
        ids: seats.iter().map(|s| s.id.clone()).collect(),
        stats: st,
        final_tick: result.final_tick,
        closer,
        slowest_ms: slowest,
        mean_ms: total / moves.max(1) as f64,
    }
}

fn side(g: &Game, id: &str) -> Side {
    let mut s = Side::default();
    for (i, x) in g.stats.iter().enumerate() {
        if g.ids[i] != id {
            continue;
        }
        s.seats += 1.0;
        s.score += x.score;
        s.avp += x.avp;
        s.closes += x.closes;
        s.enemy_caps += x.enemy_caps;
        s.early_caps += x.early_caps;
        s.neutral += x.neutral;
        s.lost += x.lost;
        s.surrendered += x.surrendered;
        s.kills += x.kills;
        s.kills_strong += x.kills_strong;
        s.deaths += x.deaths;
        s.land += x.land;
        s.bld_caps += x.bld_caps;
        s.land3000 += x.land3000;
    }
    s
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
    let cand = Style { id: "candidate".into(), ..seat(&a[0]) };
    let base = Style { id: "base".into(), ..seat(&a[1]) };
    let pairs: usize = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(4);
    let seed_base: u64 = a.get(3).and_then(|x| x.parse().ok()).unwrap_or(500_000);
    let first: usize = a.get(4).and_then(|x| x.parse().ok()).unwrap_or(0);
    let pool = roster();
    let mut lobbies = vec![];
    for k in first..pairs {
        let seed = seed_base.wrapping_add(k as u64);
        let mut rng = Rng(seed ^ 0x5EA7);
        let mut seats = vec![cand.clone(); 3];
        seats.extend(vec![base.clone(); 3]);
        for _ in 0..LOBBY - 6 {
            seats.push(pool[rng.below(pool.len())].clone());
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
        lobbies.push((seed, seats));
        lobbies.push((seed, mirrored));
    }
    let threads: usize = std::env::var("HEXARENA_EVAL_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Game>>> = Mutex::new((0..lobbies.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((seed, seats)) = lobbies.get(i) else { break };
                let g = run(*seed, seats);
                let c = side(&g, "candidate");
                let b = side(&g, "base");
                eprintln!(
                    "game {i} seed {seed} tick {} closer {:?} | cand score {:.2} caps {} lost {} kills {} deaths {} land {} | base score {:.2} caps {} lost {} kills {} deaths {} land {} | slowest {:.2}ms mean {:.3}ms",
                    g.final_tick,
                    g.closer.map(|c| g.ids[c].clone()),
                    c.score / c.seats, c.enemy_caps, c.lost, c.kills, c.deaths, c.land,
                    b.score / b.seats, b.enemy_caps, b.lost, b.kills, b.deaths, b.land,
                    g.slowest_ms, g.mean_ms
                );
                out.lock().unwrap()[i] = Some(g);
            });
        }
    });
    let games: Vec<Game> = out.into_inner().unwrap().into_iter().map(|g| g.unwrap()).collect();
    let gains: Vec<f64> = games
        .chunks(2)
        .map(|p| {
            let m = |g: &Game, id: &str| {
                let s = side(g, id);
                s.score / s.seats
            };
            (m(&p[0], "base") + m(&p[1], "base")) / 2.0 - (m(&p[0], "candidate") + m(&p[1], "candidate")) / 2.0
        })
        .collect();
    let nf = gains.len() as f64;
    let mean = gains.iter().sum::<f64>() / nf;
    let se = (gains.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (nf - 1.0).max(1.0) / nf).sqrt();
    println!("gain {mean:+.3} ± {se:.3} over {} games", games.len());
    for id in ["candidate", "base"] {
        let mut t = Side::default();
        for g in &games {
            let s = side(g, id);
            t.seats += s.seats;
            t.score += s.score;
            t.avp += s.avp;
            t.closes += s.closes;
            t.enemy_caps += s.enemy_caps;
            t.early_caps += s.early_caps;
            t.neutral += s.neutral;
            t.lost += s.lost;
            t.surrendered += s.surrendered;
            t.kills += s.kills;
            t.kills_strong += s.kills_strong;
            t.deaths += s.deaths;
            t.land += s.land;
            t.bld_caps += s.bld_caps;
            t.land3000 += s.land3000;
        }
        let k = t.seats;
        println!(
            "{id:9} score {:.3} avp {:.3} closes {} | per seat: enemy caps {:.0} (pre-3000 {:.0}) lost {:.0} net {:+.0} surrendered-to-us {:.1} V/F caps {:.0} kills {:.2} (non-roster {:.2}) deaths {:.2} land@3000 {:.0} final land {:.0}",
            t.score / k, t.avp / k, t.closes,
            t.enemy_caps / k, t.early_caps / k, t.lost / k, (t.enemy_caps + t.surrendered - t.lost) / k, t.surrendered / k,
            t.bld_caps / k, t.kills / k, t.kills_strong / k, t.deaths / k, t.land3000 / k, t.land / k
        );
    }
    let conquest = games.iter().filter(|g| g.closer.is_some()).count();
    let slowest = games.iter().map(|g| g.slowest_ms).fold(0.0, f64::max);
    let mean_ms = games.iter().map(|g| g.mean_ms).sum::<f64>() / games.len() as f64;
    println!("games ended by conquest {conquest}/{} | candidate slowest move {slowest:.2} ms, mean {mean_ms:.3} ms", games.len());
}
