//! CPU standings: placement stats plus a multiplayer Elo rating per CPU style.
//!
//! Each finished free-for-all is scored as every pair of seats played a head-to-head
//! game: the better place wins, an equal place is a draw. Seats with the same CPU
//! style are not compared with each other. A seat's rating change is
//! `K / (N - 1) * sum(actual - expected)` over its opponents, and a style with
//! several seats in one game takes the average of their changes. Ratings move only
//! after the whole game is scored, and games are applied oldest first.

use serde::Serialize;
use std::collections::BTreeMap;

pub const START_ELO: f64 = 1500.0;
pub const K: f64 = 32.0;
const HISTORY: usize = 60;

/// One seat's outcome in one finished game.
pub struct SeatResult<'a> {
    pub bot: &'a str,
    /// Average place, e.g. 2.5 for a tie across 2nd and 3rd.
    pub place: f64,
    pub normalized: f64,
    pub won: bool,
    /// Lobby size, used for the top-half rate.
    pub players: usize,
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
    pub avg_normalized: f64,
    /// Rating after each of this style's most recent games, oldest first.
    pub history: Vec<f64>,
}

fn expected(r: f64, opp: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((opp - r) / 400.0))
}

/// `games` must be in chronological order. `name` maps a style id to its display name.
pub fn standings(games: &[Vec<SeatResult>], name: impl Fn(&str) -> String) -> Vec<BotStanding> {
    let mut rows: BTreeMap<String, BotStanding> = BTreeMap::new();
    for game in games {
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
            let e = delta.entry(a.bot).or_insert((0.0, 0));
            e.0 += K / (n - 1) as f64 * d;
            e.1 += 1;
        }
        for (bot, (sum, seats)) in delta {
            let r = rows.get_mut(bot).unwrap();
            r.elo_change = sum / seats as f64;
            r.elo += r.elo_change;
            r.peak_elo = r.peak_elo.max(r.elo);
            r.games += 1;
            r.history.push(r.elo);
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
            r.avg_normalized += s.normalized;
        }
    }
    let mut out: Vec<BotStanding> = rows
        .into_values()
        .map(|mut r| {
            r.avg_place /= r.seats as f64;
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
