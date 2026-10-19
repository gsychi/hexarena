//! Head-to-head comparison of two bots in mirrored 8-player lobbies, for trying a
//! version locally before publishing it.

use crate::play_result;
use hexarena_protocol::RulesConfig;
use hexarena_sdk::Style;
use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub const LOBBY: usize = 8;
/// Places knocked off the score of a player who ends the game before the cutoff...
const CLOSE_BONUS: f64 = 0.5;
/// ...plus up to this many more, scaled by how early it ended (all of it at tick 0).
const CLOSE_SPEED_BONUS: f64 = 1.0;

/// Average placement on the 8-player scale: 1 is first, 8 is last. A shared place range
/// counts as its midpoint, and smaller lobbies are stretched onto 1..8.
pub fn avp8(place_start: u16, place_end: u16, players: usize) -> f64 {
    let place = (place_start + place_end) as f64 / 2.0;
    if players <= 1 {
        return 1.0;
    }
    1.0 + (place - 1.0) * 7.0 / (players as f64 - 1.0)
}

/// One finished lobby: who sat where and their 8-player-scale placement.
#[derive(Clone, Debug, Serialize)]
pub struct Lobby {
    pub seed: u64,
    pub ids: Vec<String>,
    pub avp: Vec<f64>,
    pub final_tick: u32,
    /// The seat that ended the game before the cutoff by being the last one standing.
    pub closer: Option<usize>,
}

/// Seats are named `Player 1`..`Player 8`, so a bot can't spot copies of itself by name
/// and team up with them against the bot it is being compared with.
fn play_lobby(seed: u64, seats: &[Style]) -> Result<Lobby, String> {
    let styles: Vec<Style> = seats
        .iter()
        .enumerate()
        .map(|(i, s)| Style { name: format!("Player {}", i + 1), blurb: String::new(), ..s.clone() })
        .collect();
    let result = play_result(seed, &styles, u32::MAX)?;
    let cutoff = RulesConfig::default().max_ticks;
    let mut avp = vec![0.0; seats.len()];
    for p in &result.placements {
        avp[p.player as usize] = avp8(p.place_start, p.place_end, seats.len());
    }
    Ok(Lobby {
        seed,
        ids: seats.iter().map(|s| s.id.clone()).collect(),
        avp,
        final_tick: result.final_tick,
        closer: (result.final_tick < cutoff).then(|| result.winner()).flatten().map(|p| p as usize),
    })
}

/// Plays every lobby on `HEXARENA_EVAL_THREADS` threads (all cores by default),
/// returning them in input order.
pub fn play_all(lobbies: &[(u64, Vec<Style>)]) -> Result<Vec<Lobby>, String> {
    let threads = std::env::var("HEXARENA_EVAL_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .or_else(|| std::thread::available_parallelism().map(|n| n.get()).ok())
        .unwrap_or(2)
        .clamp(1, lobbies.len().max(1));
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Result<Lobby, String>>>> = Mutex::new(vec![None; lobbies.len()]);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((seed, seats)) = lobbies.get(i) else { break };
                let r = play_lobby(*seed, seats);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(|r| r.unwrap()).collect()
}

/// A seat's score: its 8-player AVP, minus the closing bonus if it ended the game
/// early. Lower is better.
pub fn seat_score(lobby: &Lobby, seat: usize) -> f64 {
    let mut s = lobby.avp[seat];
    if lobby.closer == Some(seat) {
        let cutoff = RulesConfig::default().max_ticks as f64;
        let early = (cutoff - lobby.final_tick as f64).max(0.0) / cutoff;
        s -= CLOSE_BONUS + CLOSE_SPEED_BONUS * early;
    }
    s
}

fn mean_score(lobby: &Lobby, id: &str) -> f64 {
    let v: Vec<f64> = (0..lobby.ids.len()).filter(|&i| lobby.ids[i] == id).map(|i| seat_score(lobby, i)).collect();
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

#[derive(Clone, Debug, Serialize)]
pub struct Comparison {
    pub games: usize,
    /// Mean scores: 8-player AVP minus any closing bonus.
    pub candidate_score: f64,
    pub base_score: f64,
    /// Mean per-pair improvement of the candidate over the base, in places (positive = better).
    pub gain: f64,
    pub gain_se: f64,
    /// Games each side ended before the cutoff.
    pub candidate_closes: usize,
    pub base_closes: usize,
    pub secs: f64,
}

/// `candidate` vs `base` over `pairs` mirrored pairs of 8-player lobbies: 3 seats each
/// plus 2 drawn from `pool`, then the same seed again with the two swapped, so spawn
/// luck cancels out.
pub fn compare(candidate: &Style, base: &Style, pool: &[Style], seed_base: u64, pairs: usize) -> Result<(Comparison, Vec<Lobby>), String> {
    let start = Instant::now();
    let cand = Style { id: "candidate".into(), ..candidate.clone() };
    let base = Style { id: "base".into(), ..base.clone() };
    let mut lobbies = vec![];
    for k in 0..pairs {
        let seed = seed_base.wrapping_add(k as u64);
        let mut rng = Rng(seed ^ 0x5EA7);
        let mut seats = vec![cand.clone(); 3];
        seats.extend(vec![base.clone(); 3]);
        for _ in 0..LOBBY - 6 {
            seats.push(if pool.is_empty() { base.clone() } else { pool[rng.below(pool.len())].clone() });
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
    let played = play_all(&lobbies)?;
    let gains: Vec<f64> = played
        .chunks(2)
        .map(|p| {
            let c = (mean_score(&p[0], "candidate") + mean_score(&p[1], "candidate")) / 2.0;
            let b = (mean_score(&p[0], "base") + mean_score(&p[1], "base")) / 2.0;
            b - c
        })
        .collect();
    let closes = |id: &str| played.iter().filter(|l| l.closer.is_some_and(|s| l.ids[s] == id)).count();
    let mean = |id: &str| played.iter().map(|l| mean_score(l, id)).sum::<f64>() / played.len().max(1) as f64;
    let (gain, gain_se) = mean_se(&gains);
    Ok((
        Comparison {
            games: played.len(),
            candidate_score: mean("candidate"),
            base_score: mean("base"),
            gain,
            gain_se,
            candidate_closes: closes("candidate"),
            base_closes: closes("base"),
            secs: start.elapsed().as_secs_f64(),
        },
        played,
    ))
}

fn mean_se(v: &[f64]) -> (f64, f64) {
    let n = v.len() as f64;
    if n < 2.0 {
        return (v.first().copied().unwrap_or(0.0), f64::INFINITY);
    }
    let m = v.iter().sum::<f64>() / n;
    let var = v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0);
    (m, (var / n).sqrt())
}

/// SplitMix64, so lobbies are the same on every machine.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avp_is_on_the_eight_player_scale() {
        assert_eq!(avp8(1, 1, 8), 1.0);
        assert_eq!(avp8(8, 8, 8), 8.0);
        assert_eq!(avp8(2, 2, 2), 8.0);
        assert_eq!(avp8(3, 4, 8), 3.5);
        assert_eq!(avp8(3, 3, 5), 4.5);
    }

    #[test]
    fn ending_the_game_early_is_worth_more() {
        let lobby = |final_tick, closer| Lobby { seed: 0, ids: vec!["a".into(), "b".into()], avp: vec![1.0, 8.0], final_tick, closer };
        let cutoff = RulesConfig::default().max_ticks;
        assert_eq!(seat_score(&lobby(cutoff, None), 0), 1.0);
        let late = seat_score(&lobby(cutoff - 1, Some(0)), 0);
        let early = seat_score(&lobby(cutoff / 2, Some(0)), 0);
        assert!(late < 1.0 && early < late);
        assert!((early - (1.0 - CLOSE_BONUS - CLOSE_SPEED_BONUS / 2.0)).abs() < 1e-9);
        assert_eq!(seat_score(&lobby(cutoff / 2, Some(0)), 1), 8.0);
    }
}
