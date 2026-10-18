//! Placement scoring and leaderboard aggregates. Pure functions over finished matches.

use hexarena_protocol::{MatchResult, PlayerId};
use std::collections::BTreeMap;

/// (owned land, total troops, shared resource, active buildings)
pub type SurvivorKey = (u32, u32, u32, u32);

pub fn placement_points(n: usize, place: f64) -> f64 {
    n as f64 - place + 1.0
}

pub fn normalized_score(n: usize, place: f64) -> f64 {
    if n < 2 {
        1.0
    } else {
        (n as f64 - place) / (n as f64 - 1.0)
    }
}

/// Groups players best-to-worst. Survivors are ranked by `keys` (exact ties share);
/// eliminated players rank below survivors, later elimination first, same tick tied.
pub fn rank_groups(eliminated_at: &[Option<u32>], keys: &[SurvivorKey]) -> Vec<Vec<PlayerId>> {
    let n = eliminated_at.len();
    let mut survivors: BTreeMap<std::cmp::Reverse<SurvivorKey>, Vec<PlayerId>> = BTreeMap::new();
    let mut dead: BTreeMap<std::cmp::Reverse<u32>, Vec<PlayerId>> = BTreeMap::new();
    for p in 0..n {
        match eliminated_at[p] {
            None => survivors
                .entry(std::cmp::Reverse(keys[p]))
                .or_default()
                .push(p as PlayerId),
            Some(t) => dead
                .entry(std::cmp::Reverse(t))
                .or_default()
                .push(p as PlayerId),
        }
    }
    survivors.into_values().chain(dead.into_values()).collect()
}

pub fn build_result(
    tick: u32,
    eliminated_at: &[Option<u32>],
    keys: &[SurvivorKey],
    final_hexes: Vec<u32>,
) -> MatchResult {
    let n = eliminated_at.len();
    let order = rank_groups(eliminated_at, keys);
    let (mut places, mut points, mut normalized) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    let mut start = 1usize;
    for group in &order {
        let place = start as f64 + (group.len() as f64 - 1.0) / 2.0;
        for &p in group {
            places[p as usize] = place;
            points[p as usize] = placement_points(n, place);
            normalized[p as usize] = normalized_score(n, place);
        }
        start += group.len();
    }
    let winner = (order[0].len() == 1).then(|| order[0][0]);
    MatchResult {
        winner,
        ticks: tick,
        final_hexes,
        placement_order: order,
        places,
        points,
        normalized,
        survival_ticks: eliminated_at.iter().map(|e| e.unwrap_or(tick)).collect(),
    }
}

/// Metrics are never combined across these dimensions.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetricsKey {
    pub bot: String,
    pub rules_version: String,
    pub lobby_size: usize,
    pub ranked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Metrics {
    pub games: u32,
    pub sum_points: f64,
    pub sum_place: f64,
    pub sum_normalized: f64,
    pub sum_survival: f64,
    pub wins: f64,
    pub top4: f64,
    pub top_half: f64,
    pub last: f64,
    /// Fractional counts per whole place index (ties split evenly across spanned places).
    pub distribution: Vec<f64>,
    pub crashes: u32,
    pub timeouts: u32,
    pub illegal_forfeits: u32,
}

impl Metrics {
    pub fn record(&mut self, n: usize, result: &MatchResult, p: PlayerId) {
        let i = p as usize;
        let place = result.places[i];
        let group = result
            .placement_order
            .iter()
            .find(|g| g.contains(&p))
            .unwrap();
        let first = place - (group.len() as f64 - 1.0) / 2.0;
        let share = |pred: &dyn Fn(usize) -> bool| -> f64 {
            (0..group.len())
                .filter(|k| pred(first as usize + k))
                .count() as f64
                / group.len() as f64
        };
        if self.distribution.len() < n {
            self.distribution.resize(n, 0.0);
        }
        for k in 0..group.len() {
            self.distribution[first as usize - 1 + k] += 1.0 / group.len() as f64;
        }
        self.games += 1;
        self.sum_points += result.points[i];
        self.sum_place += place;
        self.sum_normalized += result.normalized[i];
        self.sum_survival += result.survival_ticks[i] as f64;
        self.wins += share(&|pl| pl == 1);
        self.top4 += share(&|pl| pl <= 4);
        self.top_half += share(&|pl| pl <= n / 2);
        self.last += share(&|pl| pl == n);
    }
    fn rate(&self, v: f64) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            v / self.games as f64
        }
    }
    pub fn avg_points(&self) -> f64 {
        self.rate(self.sum_points)
    }
    pub fn avg_place(&self) -> f64 {
        self.rate(self.sum_place)
    }
    pub fn avg_normalized(&self) -> f64 {
        self.rate(self.sum_normalized)
    }
    pub fn avg_survival(&self) -> f64 {
        self.rate(self.sum_survival)
    }
    pub fn win_rate(&self) -> f64 {
        self.rate(self.wins)
    }
    pub fn top4_rate(&self) -> f64 {
        self.rate(self.top4)
    }
    pub fn top_half_rate(&self) -> f64 {
        self.rate(self.top_half)
    }
    pub fn last_rate(&self) -> f64 {
        self.rate(self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(elim: &[Option<u32>], keys: &[SurvivorKey]) -> MatchResult {
        build_result(100, elim, keys, vec![0; elim.len()])
    }

    #[test]
    fn eight_player_points_table() {
        let elim: Vec<Option<u32>> = vec![
            None,
            Some(70),
            Some(60),
            Some(50),
            Some(40),
            Some(30),
            Some(20),
            Some(10),
        ];
        let r = res(&elim, &vec![(1, 1, 1, 1); 8]);
        assert_eq!(r.points, vec![8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0]);
        assert_eq!(r.winner, Some(0));
        assert_eq!(r.normalized[0], 1.0);
        assert_eq!(r.normalized[7], 0.0);
    }

    #[test]
    fn simultaneous_elimination_shares_average_points() {
        // players 2 and 3 eliminated on the same tick -> places 3 and 4 -> 5.5 points each
        let elim = vec![
            None,
            Some(70),
            Some(50),
            Some(50),
            Some(40),
            Some(30),
            Some(20),
            Some(10),
        ];
        let r = res(&elim, &vec![(1, 1, 1, 1); 8]);
        assert_eq!(r.points[2], 7.0 - 0.0 - 1.5);
        assert_eq!(r.points[2], 5.5);
        assert_eq!(r.points[3], 5.5);
        assert_eq!(r.places[2], 4.5 - 1.0);
    }

    #[test]
    fn survivors_ranked_by_land_troops_resource_buildings_with_exact_ties_shared() {
        let elim = vec![None, None, None, None];
        let keys = vec![(10, 5, 0, 0), (10, 6, 0, 0), (12, 0, 0, 0), (10, 6, 0, 0)];
        let r = res(&elim, &keys);
        assert_eq!(r.placement_order, vec![vec![2], vec![1, 3], vec![0]]);
        assert_eq!(r.points, vec![1.0, 2.5, 4.0, 2.5]);
        assert_eq!(r.winner, Some(2));
        let r = res(&elim, &vec![(1, 1, 1, 1); 4]);
        assert_eq!(r.winner, None);
        assert!(r.points.iter().all(|p| *p == 2.5));
    }

    #[test]
    fn surviving_longer_always_beats_dying_earlier() {
        let r = res(
            &[Some(90), Some(10), None],
            &[(0, 0, 0, 0), (0, 0, 0, 0), (1, 1, 1, 1)],
        );
        assert!(r.points[0] > r.points[1]);
    }

    #[test]
    fn metrics_aggregate() {
        let r = res(&[None, Some(5), Some(4), Some(3)], &vec![(1, 1, 1, 1); 4]);
        let mut m = Metrics::default();
        m.record(4, &r, 0);
        m.record(4, &r, 3);
        assert_eq!(m.games, 2);
        assert_eq!(m.win_rate(), 0.5);
        assert_eq!(m.last_rate(), 0.5);
        assert_eq!(m.top_half_rate(), 0.5);
        assert_eq!(m.top4_rate(), 1.0);
        assert_eq!(m.avg_points(), 2.5);
        assert_eq!(m.avg_normalized(), 0.5);
        assert_eq!(m.distribution, vec![1.0, 0.0, 0.0, 1.0]);
    }
}
