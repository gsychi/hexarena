//! CPU standings: placement stats plus a multiplayer Elo rating per CPU style.
//!
//! Each finished free-for-all is scored as every pair of seats played a head-to-head
//! game: the better place wins, an equal place is a draw. Seats with the same CPU
//! style are not compared with each other. A seat's rating change is
//! `K / (N - 1) * sum(actual - expected)` over its opponents, and a style with
//! several seats in one game takes the average of their changes. A style's first
//! [`PROVISIONAL_GAMES`] games use [`PROVISIONAL_K`] times K, and every game is weighted
//! by [`game_weight`]: one ended before the cutoff counts N times a cutoff game. Ratings
//! move only after the whole game is scored, and games are applied oldest first.

use serde::Serialize;
use std::collections::BTreeMap;

pub const START_ELO: f64 = 1500.0;
/// The first CPUs ever written, the yardstick every rating is measured against. Their
/// average stays at [`START_ELO`]: after each game the anchors' net change is taken
/// back evenly from all of them, so they still spread out against each other but the
/// group never drifts. One plays in every self-play game, so ratings rise as bots get
/// better instead of only ranking the field against itself.
pub const ANCHORS: [&str; 4] = ["expander", "homesteader", "raider", "warden"];

pub fn is_anchor(bot: &str) -> bool {
    ANCHORS.contains(&bot)
}
pub const K: f64 = 32.0;
/// A bot's first games move its rating this many times as far, so a new version reaches
/// its real level before it can be retired for its ranking.
pub const PROVISIONAL_K: f64 = 3.0;
pub const PROVISIONAL_GAMES: u32 = 20;
/// Games before a rating counts as settled. One 8-player place varies by about 2.2
/// places, so 50 games pin the average to about +-0.3 places (one standard error),
/// enough to tell apart bots half a place to a place apart.
pub const RATED_GAMES: u32 = 50;
const HISTORY: usize = 60;

/// How far a finished game moves ratings, as a multiple of K. A game someone ended
/// before the cutoff counts as many times as a cutoff game as there were players, so
/// closing out an 8-player lobby is worth 8 times coasting to the cutoff in one. The
/// ended weight stays at 2, so ratings are as steady as before when most games end.
pub fn game_weight(ended: bool, players: usize) -> f64 {
    const ENDED: f64 = 2.0;
    if ended { ENDED } else { ENDED / players.max(1) as f64 }
}

/// One seat's outcome in one finished game.
pub struct SeatResult<'a> {
    pub bot: &'a str,
    /// Average place, e.g. 2.5 for a tie across 2nd and 3rd.
    pub place: f64,
    pub normalized: f64,
    pub won: bool,
    /// Lobby size, used for the top-half rate.
    pub players: usize,
    /// The game ended before the cutoff, with one player left standing.
    pub ended: bool,
    /// Average time per move in this game, when it was timed.
    pub ms_per_move: Option<f64>,
    /// Slowest single move in this game, when it was timed.
    pub slowest_ms: Option<f64>,
}

#[derive(Serialize, Default, Debug)]
pub struct BotStanding {
    pub rank: u32,
    pub bot: String,
    pub name: String,
    pub elo: f64,
    pub elo_change: f64,
    pub peak_elo: f64,
    pub games: u32,
    pub seats: u32,
    pub wins: u32,
    /// Share of seats that finished first outright.
    pub win_rate: f64,
    pub top_half: u32,
    /// Share of seats whose average place is in the top half (`place <= N / 2`).
    pub top_half_rate: f64,
    pub avg_place: f64,
    /// Average place stretched onto the 8-player scale (1 best, 8 worst), so lobbies of
    /// every size compare like 8-player games.
    pub avp8: f64,
    /// Standard error of `avp8`: how far the true average place might be from it.
    pub avp8_se: f64,
    /// Played [`RATED_GAMES`] games, so the rating can be trusted.
    pub rated: bool,
    #[serde(skip)]
    avp8_sq: f64,
    /// Average time per move: the mean of its per-game averages over timed games.
    pub ms_per_move: Option<f64>,
    /// Half-width of the 95% confidence interval of `ms_per_move` (t-distribution over
    /// timed games, which are independent where single moves within a game are not).
    pub ms_per_move_ci95: Option<f64>,
    /// Slowest single move in any timed game.
    pub slowest_ms: Option<f64>,
    /// Seats with timing, the sample size behind `ms_per_move`.
    pub timed_games: u32,
    #[serde(skip)]
    ms_sum: f64,
    #[serde(skip)]
    ms_sq: f64,
    pub avg_normalized: f64,
    /// Rating after each of this style's most recent games, oldest first.
    pub history: Vec<f64>,
    /// Every rated game this style played, as (index into the games list, rating after).
    #[serde(skip)]
    pub timeline: Vec<(usize, f64)>,
}

/// Two-sided 95% Student-t critical value for `df` degrees of freedom (exact to 10,
/// then within 0.03 of the true value, tending to 1.96).
fn t95(df: u32) -> f64 {
    const T: [f64; 10] = [12.71, 4.30, 3.18, 2.78, 2.57, 2.45, 2.36, 2.31, 2.26, 2.23];
    match df {
        0 => f64::INFINITY,
        1..=10 => T[df as usize - 1],
        _ => 1.96 + 2.4 / df as f64,
    }
}

fn expected(r: f64, opp: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((opp - r) / 400.0))
}

/// `games` must be in chronological order. `name` maps a style id to its display name.
pub fn standings(games: &[Vec<SeatResult>], name: impl Fn(&str) -> String) -> Vec<BotStanding> {
    let mut rows: BTreeMap<String, BotStanding> = BTreeMap::new();
    for (gi, game) in games.iter().enumerate() {
        let n = game.len();
        if n < 2 {
            continue;
        }
        for s in game {
            rows.entry(s.bot.to_string())
                .or_insert_with(|| BotStanding {
                    bot: s.bot.to_string(),
                    name: name(s.bot),
                    elo: START_ELO,
                    peak_elo: START_ELO,
                    history: vec![START_ELO],
                    ..Default::default()
                });
        }
        let rating: Vec<f64> = game.iter().map(|s| rows[s.bot].elo).collect();
        let mut delta: BTreeMap<&str, (f64, u32)> = BTreeMap::new();
        for (i, a) in game.iter().enumerate() {
            let mut d = 0.0;
            for (j, b) in game.iter().enumerate() {
                if i == j || a.bot == b.bot {
                    continue;
                }
                let actual = if a.place < b.place {
                    1.0
                } else if a.place == b.place {
                    0.5
                } else {
                    0.0
                };
                d += actual - expected(rating[i], rating[j]);
            }
            let k = if rows[a.bot].games < PROVISIONAL_GAMES { K * PROVISIONAL_K } else { K };
            let k = k * game_weight(a.ended, n);
            let e = delta.entry(a.bot).or_insert((0.0, 0));
            e.0 += k / (n - 1) as f64 * d;
            e.1 += 1;
        }
        let anchored: Vec<String> = rows.keys().filter(|b| is_anchor(b)).cloned().collect();
        let drift = delta.iter().filter(|(b, _)| is_anchor(b)).map(|(_, (sum, seats))| sum / *seats as f64).sum::<f64>()
            / anchored.len().max(1) as f64;
        for b in anchored.iter().filter(|b| !delta.contains_key(b.as_str())) {
            rows.get_mut(b).unwrap().elo -= drift;
        }
        for (bot, (sum, seats)) in delta {
            let r = rows.get_mut(bot).unwrap();
            r.elo_change = sum / seats as f64 - if is_anchor(bot) { drift } else { 0.0 };
            r.elo += r.elo_change;
            r.peak_elo = r.peak_elo.max(r.elo);
            r.games += 1;
            r.history.push(r.elo);
            r.timeline.push((gi, r.elo));
            if r.history.len() > HISTORY {
                r.history.remove(0);
            }
        }
        for s in game {
            let r = rows.get_mut(s.bot).unwrap();
            r.seats += 1;
            r.wins += s.won as u32;
            r.top_half += (s.place <= s.players as f64 / 2.0) as u32;
            r.avg_place += s.place;
            let avp8 = 1.0 + (s.place - 1.0) * 7.0 / (s.players as f64 - 1.0);
            r.avp8 += avp8;
            r.avp8_sq += avp8 * avp8;
            r.avg_normalized += s.normalized;
            if let Some(ms) = s.ms_per_move {
                r.timed_games += 1;
                r.ms_sum += ms;
                r.ms_sq += ms * ms;
            }
            if let Some(ms) = s.slowest_ms {
                r.slowest_ms = Some(r.slowest_ms.unwrap_or(0.0).max(ms));
            }
        }
    }
    let mut out: Vec<BotStanding> = rows
        .into_values()
        .map(|mut r| {
            r.avg_place /= r.seats as f64;
            let n = r.seats as f64;
            r.avp8 /= n;
            let var = (r.avp8_sq / n - r.avp8 * r.avp8).max(0.0) * n / (n - 1.0).max(1.0);
            r.avp8_se = (var / n).sqrt();
            r.rated = r.games >= RATED_GAMES;
            if r.timed_games > 0 {
                let n = r.timed_games as f64;
                let mean = r.ms_sum / n;
                r.ms_per_move = Some(mean);
                if r.timed_games > 1 {
                    let var = (r.ms_sq / n - mean * mean).max(0.0) * n / (n - 1.0);
                    r.ms_per_move_ci95 = Some(t95(r.timed_games - 1) * (var / n).sqrt());
                }
            }
            r.avg_normalized /= r.seats as f64;
            r.win_rate = r.wins as f64 / r.seats as f64;
            r.top_half_rate = r.top_half as f64 / r.seats as f64;
            r
        })
        .collect();
    out.sort_by(|a, b| b.elo.total_cmp(&a.elo).then(a.bot.cmp(&b.bot)));
    for (i, r) in out.iter_mut().enumerate() {
        r.rank = i as u32 + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(bot: &str, place: f64) -> SeatResult<'_> {
        SeatResult {
            bot,
            place,
            normalized: 0.0,
            won: place == 1.0,
            players: 3,
            ended: false,
            ms_per_move: None,
            slowest_ms: None,
        }
    }

    #[test]
    fn winner_gains_loser_drops_and_total_is_conserved() {
        let games = vec![vec![seat("a", 1.0), seat("b", 2.0), seat("c", 3.0)]];
        let t = standings(&games, |s| s.to_string());
        let by = |b: &str| t.iter().find(|r| r.bot == b).unwrap();
        assert!(by("a").elo > START_ELO && by("c").elo < START_ELO);
        assert!((by("b").elo - START_ELO).abs() < 1e-9);
        let total: f64 = t.iter().map(|r| r.elo).sum();
        assert!((total - 3.0 * START_ELO).abs() < 1e-9);
        assert_eq!(t[0].rank, 1);
        assert_eq!(t[0].bot, "a");
    }

    #[test]
    fn anchors_average_stays_put_so_beating_them_raises_the_scale() {
        let games: Vec<_> = (0..30)
            .flat_map(|_| {
                [
                    vec![seat("a", 1.0), seat("b", 2.0), seat("warden", 3.0)],
                    vec![seat("a", 1.0), seat("b", 2.0), seat("expander", 3.0)],
                    vec![seat("expander", 1.0), seat("warden", 2.0)],
                ]
            })
            .collect();
        let t = standings(&games, |s| s.to_string());
        let by = |b: &str| t.iter().find(|r| r.bot == b).unwrap().elo;
        assert!(((by("warden") + by("expander")) / 2.0 - START_ELO).abs() < 1e-9);
        assert!(by("expander") > by("warden"));
        assert!(by("a") > START_ELO && by("b") > START_ELO);
    }

    #[test]
    fn top_half_counts_places_up_to_half_the_lobby() {
        let mut four: Vec<_> = [1.0, 2.0, 3.0, 4.0]
            .iter()
            .zip(["a", "b", "c", "d"])
            .map(|(&p, b)| seat(b, p))
            .collect();
        four.iter_mut().for_each(|s| s.players = 4);
        let mut five: Vec<_> = [1.0, 2.5, 2.5, 4.0, 5.0]
            .iter()
            .zip(["a", "b", "c", "d", "e"])
            .map(|(&p, b)| seat(b, p))
            .collect();
        five.iter_mut().for_each(|s| s.players = 5);
        let t = standings(&[four, five], |s| s.to_string());
        let rate = |b: &str| t.iter().find(|r| r.bot == b).unwrap().top_half_rate;
        assert_eq!(rate("a"), 1.0);
        assert_eq!(rate("b"), 1.0);
        assert_eq!(rate("c"), 0.5);
        assert_eq!(rate("d"), 0.0);
    }

    #[test]
    fn ties_are_draws_and_mirror_seats_are_not_compared() {
        let games = vec![
            vec![seat("a", 1.5), seat("b", 1.5)],
            vec![seat("a", 1.0), seat("a", 2.0)],
        ];
        let t = standings(&games, |s| s.to_string());
        for r in &t {
            assert!((r.elo - START_ELO).abs() < 1e-9);
        }
    }

    #[test]
    fn closing_a_game_out_moves_ratings_lobby_size_times_as_far() {
        let lobby = |ended: bool| -> Vec<SeatResult<'static>> {
            ["a", "b", "c", "d", "e", "f", "g", "h"]
                .iter()
                .enumerate()
                .map(|(i, b)| SeatResult { players: 8, ended, ..seat(b, i as f64 + 1.0) })
                .collect()
        };
        let gain = |ended: bool| {
            let t = standings(&[lobby(ended)], |s| s.to_string());
            t.iter().find(|r| r.bot == "a").unwrap().elo - START_ELO
        };
        assert!(gain(false) > 0.0);
        assert!((gain(true) / gain(false) - 8.0).abs() < 1e-9);
    }

    #[test]
    fn move_time_is_averaged_over_games_with_a_95_percent_interval() {
        let timed = |ms: Option<f64>| vec![
            SeatResult { ms_per_move: ms, slowest_ms: ms.map(|m| m * 10.0), ..seat("a", 1.0) },
            seat("b", 2.0),
        ];
        let games = vec![timed(Some(1.0)), timed(Some(3.0)), timed(None)];
        let t = standings(&games, |s| s.to_string());
        let a = t.iter().find(|r| r.bot == "a").unwrap();
        assert_eq!((a.timed_games, a.ms_per_move, a.slowest_ms), (2, Some(2.0), Some(30.0)));
        // sd sqrt(2), se 1, t(1) = 12.71
        assert!((a.ms_per_move_ci95.unwrap() - 12.71).abs() < 1e-9);
        let b = t.iter().find(|r| r.bot == "b").unwrap();
        assert_eq!((b.timed_games, b.ms_per_move, b.ms_per_move_ci95), (0, None, None));
    }

    #[test]
    fn repeated_wins_have_diminishing_returns() {
        let games: Vec<_> = (0..5)
            .map(|_| vec![seat("a", 1.0), seat("b", 2.0)])
            .collect();
        let t = standings(&games, |s| s.to_string());
        let h = &t.iter().find(|r| r.bot == "a").unwrap().history;
        assert!(h[1] - h[0] > h[5] - h[4]);
        assert_eq!(h.len(), 6);
    }
}
