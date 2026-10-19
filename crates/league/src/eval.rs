//! Fast parallel evaluation in 8-player lobbies.

use crate::rng::Rng;
use hexarena_arena::play_result;
use hexarena_protocol::RulesConfig;
use hexarena_sdk::process::Program;
use hexarena_sdk::{BotParams, Style};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub const LOBBY: usize = 8;

/// Average placement on the 8-player scale: 1 is first, 8 is last. A shared place range
/// counts as its midpoint, and smaller lobbies are stretched onto 1..8.
pub fn avp8(place_start: u16, place_end: u16, players: usize) -> f64 {
    let place = (place_start + place_end) as f64 / 2.0;
    if players <= 1 {
        return 1.0;
    }
    1.0 + (place - 1.0) * 7.0 / (players as f64 - 1.0)
}

/// One seat's bot: an identity for ratings plus the genome it plays.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entrant {
    pub id: String,
    pub params: BotParams,
    /// The program that plays it, for bots built outside this binary (arena versions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
}

impl Entrant {
    pub fn new(id: impl Into<String>, params: BotParams) -> Entrant {
        Entrant { id: id.into(), params, program: None }
    }
}

/// One finished lobby: who sat where and their 8-player-scale placement.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameLog {
    pub seed: u64,
    pub ids: Vec<String>,
    pub avp: Vec<f64>,
    /// When the game finished (ms since the epoch); 0 in logs written before it was recorded.
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub final_tick: u32,
    /// The seat that ended the game before the cutoff by being the last one standing.
    #[serde(default)]
    pub closer: Option<usize>,
}

/// Seats are named `Player 1`..`Player 8`, so a bot can't spot copies of itself by name
/// and team up with them against the bot it is being compared with.
pub fn play_lobby(seed: u64, seats: &[Entrant]) -> Result<GameLog, String> {
    let styles: Vec<Style> = seats
        .iter()
        .enumerate()
        .map(|(i, e)| Style {
            id: e.id.clone(),
            name: format!("Player {}", i + 1),
            blurb: String::new(),
            params: e.params.clone(),
            program: e.program.clone(),
        })
        .collect();
    let result = play_result(seed, &styles, u32::MAX)?;
    let cutoff = RulesConfig::default().max_ticks;
    let mut avp = vec![0.0; seats.len()];
    for p in &result.placements {
        avp[p.player as usize] = avp8(p.place_start, p.place_end, seats.len());
    }
    Ok(GameLog {
        seed,
        ids: seats.iter().map(|e| e.id.clone()).collect(),
        avp,
        at: crate::now_ms(),
        final_tick: result.final_tick,
        closer: (result.final_tick < cutoff).then(|| result.winner()).flatten().map(|p| p as usize),
    })
}

/// Plays every lobby on all cores, returning logs in input order.
pub fn play_all(lobbies: &[(u64, Vec<Entrant>)]) -> Result<Vec<GameLog>, String> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(lobbies.len().max(1));
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<Option<Result<GameLog, String>>>> =
        Mutex::new(vec![None; lobbies.len()]);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((seed, seats)) = lobbies.get(i) else {
                    break;
                };
                let r = play_lobby(*seed, seats);
                out.lock().unwrap()[i] = Some(r);
            });
        }
    });
    out.into_inner().unwrap().into_iter().map(|r| r.unwrap()).collect()
}

/// A seat's gate score: its 8-player AVP, minus the closing bonus if it ended the game
/// early. Lower is better.
pub fn seat_score(log: &GameLog, seat: usize, cfg: &GateConfig) -> f64 {
    let mut s = log.avp[seat];
    if log.closer == Some(seat) {
        let cutoff = RulesConfig::default().max_ticks as f64;
        let early = (cutoff - log.final_tick as f64).max(0.0) / cutoff;
        s -= cfg.close_bonus + cfg.close_speed_bonus * early;
    }
    s
}

fn mean_score(log: &GameLog, id: &str, cfg: &GateConfig) -> f64 {
    let v: Vec<f64> = (0..log.ids.len())
        .filter(|&i| log.ids[i] == id)
        .map(|i| seat_score(log, i, cfg))
        .collect();
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// How promotion is decided. Games come in mirrored pairs on the same map, with the
/// challenger and champion seats swapped, so spawn luck cancels out.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GateConfig {
    pub min_pairs: usize,
    pub max_pairs: usize,
    pub batch_pairs: usize,
    /// Smallest AVP improvement (8-player places) worth promoting.
    pub min_gain: f64,
    /// Standard errors of improvement required at the end of the gate.
    pub z_accept: f64,
    /// Places knocked off the score of a player who ends the game before the cutoff...
    pub close_bonus: f64,
    /// ...plus up to this many more, scaled by how early it ended (all of it at tick 0).
    pub close_speed_bonus: f64,
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig {
            min_pairs: 8,
            max_pairs: 48,
            batch_pairs: 8,
            min_gain: 0.05,
            z_accept: 2.0,
            close_bonus: 0.5,
            close_speed_bonus: 1.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateResult {
    pub games: usize,
    /// Mean scores: 8-player AVP minus any closing bonus.
    pub challenger_avp: f64,
    pub champion_avp: f64,
    /// Mean per-pair AVP gain of the challenger over the champion (positive = better).
    pub gain: f64,
    pub gain_se: f64,
    pub promoted: bool,
    pub secs: f64,
}

/// Challenger vs champion in 8-player lobbies: 3 seats each plus 2 seats drawn from `pool`.
pub fn gate(
    challenger: &Entrant,
    champion: &Entrant,
    pool: &[Entrant],
    seed_base: u64,
    cfg: &GateConfig,
) -> Result<(GateResult, Vec<GameLog>), String> {
    let start = Instant::now();
    let mut logs = vec![];
    let mut gains: Vec<f64> = vec![];
    let (mut c_sum, mut h_sum) = (0.0, 0.0);
    while gains.len() < cfg.max_pairs {
        let mut lobbies = vec![];
        for k in 0..cfg.batch_pairs {
            let seed = seed_base.wrapping_add((gains.len() + k) as u64);
            let mut rng = Rng::new(seed ^ 0x5EA7);
            let mut seats = vec![challenger.clone(); 3];
            seats.extend(vec![champion.clone(); 3]);
            for _ in 0..LOBBY - 6 {
                seats.push(if pool.is_empty() {
                    champion.clone()
                } else {
                    pool[rng.below(pool.len())].clone()
                });
            }
            rng.shuffle(&mut seats);
            let mirrored: Vec<Entrant> = seats
                .iter()
                .map(|e| {
                    if e.id == challenger.id {
                        champion.clone()
                    } else if e.id == champion.id {
                        challenger.clone()
                    } else {
                        e.clone()
                    }
                })
                .collect();
            lobbies.push((seed, seats));
            lobbies.push((seed, mirrored));
        }
        let batch = play_all(&lobbies)?;
        for pair in batch.chunks(2) {
            let (c, h) = (
                (mean_score(&pair[0], &challenger.id, cfg) + mean_score(&pair[1], &challenger.id, cfg)) / 2.0,
                (mean_score(&pair[0], &champion.id, cfg) + mean_score(&pair[1], &champion.id, cfg)) / 2.0,
            );
            c_sum += c;
            h_sum += h;
            gains.push(h - c);
        }
        logs.extend(batch);
        let n = gains.len();
        let (m, se) = mean_se(&gains);
        let hopeless = m + se < 0.0 || (n >= cfg.max_pairs / 2 && m + 2.0 * se < cfg.min_gain);
        let clear_win = m >= cfg.min_gain && m > (cfg.z_accept + 1.0) * se;
        // Every pair tied exactly: the changed genes are inert for this genome.
        let inert = gains.iter().all(|g| *g == 0.0);
        if n >= cfg.min_pairs && (hopeless || clear_win || inert) {
            break;
        }
    }
    let n = gains.len();
    let (gain, gain_se) = mean_se(&gains);
    Ok((
        GateResult {
            games: logs.len(),
            challenger_avp: c_sum / n as f64,
            champion_avp: h_sum / n as f64,
            gain,
            gain_se,
            promoted: gain >= cfg.min_gain && gain > cfg.z_accept * gain_se,
            secs: start.elapsed().as_secs_f64(),
        },
        logs,
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

/// Absolute strength: 2 seats of `entrant` against 6 seats of the hand-written roster on
/// the same fixed seeds every time, so generations are directly comparable.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Benchmark {
    pub games: usize,
    pub avp: f64,
    pub win_rate: f64,
    pub top_half_rate: f64,
    /// Share of benchmark games this entrant ended before the cutoff.
    #[serde(default)]
    pub close_rate: f64,
    /// Average tick of those early endings.
    #[serde(default)]
    pub avg_close_tick: Option<f64>,
}

pub const BENCH_GAMES: u64 = 32;

pub fn benchmark(entrant: &Entrant, roster: &[Entrant]) -> Result<(Benchmark, Vec<GameLog>), String> {
    let lobbies: Vec<(u64, Vec<Entrant>)> = (1..=BENCH_GAMES)
        .map(|seed| {
            let mut seats: Vec<Entrant> = (0..LOBBY - 2)
                .map(|i| roster[i % roster.len()].clone())
                .collect();
            seats.push(entrant.clone());
            seats.push(entrant.clone());
            Rng::new(seed ^ 0xBE4C).shuffle(&mut seats);
            (1_000_000 + seed, seats)
        })
        .collect();
    let logs = play_all(&lobbies)?;
    let mine: Vec<f64> = logs
        .iter()
        .flat_map(|l| l.ids.iter().zip(&l.avp).filter(|(i, _)| **i == entrant.id).map(|(_, a)| *a))
        .collect();
    let n = mine.len() as f64;
    let closes: Vec<f64> = logs
        .iter()
        .filter(|l| l.closer.is_some_and(|s| l.ids[s] == entrant.id))
        .map(|l| l.final_tick as f64)
        .collect();
    Ok((
        Benchmark {
            games: logs.len(),
            avp: mine.iter().sum::<f64>() / n,
            win_rate: mine.iter().filter(|a| **a <= 1.0).count() as f64 / n,
            top_half_rate: mine.iter().filter(|a| **a <= 4.5).count() as f64 / n,
            close_rate: closes.len() as f64 / logs.len() as f64,
            avg_close_tick: (!closes.is_empty()).then(|| closes.iter().sum::<f64>() / closes.len() as f64),
        },
        logs,
    ))
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
        let cfg = GateConfig::default();
        let log = |final_tick, closer| GameLog {
            seed: 0,
            ids: vec!["a".into(), "b".into()],
            avp: vec![1.0, 8.0],
            at: 0,
            final_tick,
            closer,
        };
        let cutoff = RulesConfig::default().max_ticks;
        assert_eq!(seat_score(&log(cutoff, None), 0, &cfg), 1.0);
        let late = seat_score(&log(cutoff - 1, Some(0)), 0, &cfg);
        let early = seat_score(&log(cutoff / 2, Some(0)), 0, &cfg);
        assert!(late < 1.0 && early < late);
        assert!((early - (1.0 - cfg.close_bonus - cfg.close_speed_bonus / 2.0)).abs() < 1e-9);
        assert_eq!(seat_score(&log(cutoff / 2, Some(0)), 1, &cfg), 8.0);
    }
}
