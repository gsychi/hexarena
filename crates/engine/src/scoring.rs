//! Placement scoring and leaderboard aggregates. Pure functions over finished matches.

use hexarena_protocol::{FixedScore, MatchResult, Placement, PlayerId, MILLI};
use std::collections::BTreeMap;

/// (owned land, total troops, shared resource, active buildings)
pub type SurvivorKey = (u32, u64, u32, u32);

/// Milli-points for a group occupying places `start..=end` of `n`: the rounded average of
/// `n - r + 1`.
pub fn group_points(n: usize, start: usize, end: usize) -> FixedScore {
    let k = (end - start + 1) as u64;
    let sum: u64 = (start..=end).map(|r| (n - r + 1) as u64).sum();
    ((sum * MILLI as u64 * 2 + k) / (2 * k)) as FixedScore
}

pub fn normalized_score(n: usize, place: f64) -> f64 {
    if n < 2 { 1.0 } else { (n as f64 - place) / (n as f64 - 1.0) }
}

/// Groups players best-to-worst. Survivors are ranked by `keys` (exact ties share);
/// eliminated players rank below survivors, later elimination first, same tick tied.
pub fn rank_groups(eliminated_at: &[Option<u32>], keys: &[SurvivorKey]) -> Vec<Vec<PlayerId>> {
    let mut survivors: BTreeMap<std::cmp::Reverse<SurvivorKey>, Vec<PlayerId>> = BTreeMap::new();
    let mut dead: BTreeMap<std::cmp::Reverse<u32>, Vec<PlayerId>> = BTreeMap::new();
    for p in 0..eliminated_at.len() {
        match eliminated_at[p] {
            None => survivors.entry(std::cmp::Reverse(keys[p])).or_default().push(p as PlayerId),
            Some(t) => dead.entry(std::cmp::Reverse(t)).or_default().push(p as PlayerId),
        }
    }
    survivors.into_values().chain(dead.into_values()).collect()
}

#[allow(clippy::too_many_arguments)]
pub fn build_result(
    version: &str,
    tick: u32,
    eliminated_at: &[Option<u32>],
    keys: &[SurvivorKey],
    land: &[u32],
    troops: &[u64],
    resource: &[u32],
    buildings: &[u32],
) -> MatchResult {
    let n = eliminated_at.len();
    let mut placements = vec![];
    let mut start = 1usize;
    for group in rank_groups(eliminated_at, keys) {
        let end = start + group.len() - 1;
        let points = group_points(n, start, end);
        for p in group {
            let i = p as usize;
            placements.push(Placement {
                player: p,
                place_start: start as u16,
                place_end: end as u16,
                points,
                eliminated_at: eliminated_at[i],
                land: land[i],
                troops: troops[i],
                resource: resource[i],
                buildings: buildings[i],
            });
        }
        start = end + 1;
    }
    MatchResult { rules_version: version.to_string(), final_tick: tick, placements }
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
    /// Fractional counts per place (ties split evenly across spanned places).
    pub distribution: Vec<f64>,
    pub crashes: u32,
    pub timeouts: u32,
    pub illegal_forfeits: u32,
}

impl Metrics {
    pub fn record(&mut self, result: &MatchResult, p: PlayerId) {
        let n = result.placements.len();
        let pl = result.placement(p);
        let (s, e) = (pl.place_start as usize, pl.place_end as usize);
        let k = (e - s + 1) as f64;
        if self.distribution.len() < n {
            self.distribution.resize(n, 0.0);
        }
        let share = |pred: &dyn Fn(usize) -> bool| (s..=e).filter(|r| pred(*r)).count() as f64 / k;
        for r in s..=e {
            self.distribution[r - 1] += 1.0 / k;
        }
        self.games += 1;
        self.sum_points += result.points_f64(p);
        self.sum_place += result.place_f64(p);
        self.sum_normalized += result.normalized(p);
        self.sum_survival += result.survival_ticks(p) as f64;
        self.wins += share(&|r| r == 1);
        self.top4 += share(&|r| r <= 4);
        self.top_half += share(&|r| r <= n / 2);
        self.last += share(&|r| r == n);
    }
    fn rate(&self, v: f64) -> f64 {
        if self.games == 0 { 0.0 } else { v / self.games as f64 }
    }
    pub fn avg_points(&self) -> f64 { self.rate(self.sum_points) }
    pub fn avg_place(&self) -> f64 { self.rate(self.sum_place) }
    pub fn avg_normalized(&self) -> f64 { self.rate(self.sum_normalized) }
    pub fn avg_survival(&self) -> f64 { self.rate(self.sum_survival) }
    pub fn win_rate(&self) -> f64 { self.rate(self.wins) }
    pub fn top4_rate(&self) -> f64 { self.rate(self.top4) }
    pub fn top_half_rate(&self) -> f64 { self.rate(self.top_half) }
    pub fn last_rate(&self) -> f64 { self.rate(self.last) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(elim: &[Option<u32>], keys: &[SurvivorKey]) -> MatchResult {
        let n = elim.len();
        build_result("v0", 100, elim, keys, &vec![0; n], &vec![0; n], &vec![0; n], &vec![0; n])
    }
    fn k() -> SurvivorKey { (1, 1, 1, 1) }

    #[test]
    fn eight_player_points_table() {
        let elim: Vec<Option<u32>> = vec![None, Some(70), Some(60), Some(50), Some(40), Some(30), Some(20), Some(10)];
        let r = res(&elim, &vec![k(); 8]);
        let pts: Vec<f64> = (0..8).map(|p| r.points_f64(p)).collect();
        assert_eq!(pts, vec![8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0]);
        assert_eq!(r.winner(), Some(0));
        assert_eq!(r.normalized(0), 1.0);
        assert_eq!(r.normalized(7), 0.0);
    }

    #[test]
    fn tied_placements_average_points() {
        let elim = vec![None, Some(70), Some(50), Some(50), Some(40), Some(30), Some(20), Some(10)];
        let r = res(&elim, &vec![k(); 8]);
        assert_eq!(r.points_f64(2), 5.5);
        assert_eq!(r.points_f64(3), 5.5);
        assert_eq!((r.placement(2).place_start, r.placement(2).place_end), (3, 4));
        assert_eq!(r.placement(2).points, 5500);
    }

    #[test]
    fn survivors_ranked_by_land_troops_resource_buildings_with_exact_ties_shared() {
        let elim = vec![None; 4];
        let keys = vec![(10, 5, 0, 0), (10, 6, 0, 0), (12, 0, 0, 0), (10, 6, 0, 0)];
        let r = res(&elim, &keys);
        let order: Vec<PlayerId> = r.placements.iter().map(|p| p.player).collect();
        assert_eq!(order[0], 2);
        assert_eq!(order[3], 0);
        assert_eq!((r.points_f64(0), r.points_f64(1), r.points_f64(2), r.points_f64(3)), (1.0, 2.5, 4.0, 2.5));
        assert_eq!(r.winner(), Some(2));
        let r = res(&elim, &vec![k(); 4]);
        assert_eq!(r.winner(), None);
        assert!((0..4).all(|p| r.points_f64(p) == 2.5));
        let two = vec![None; 2];
        let by_resource = res(&two, &[(5, 5, 1, 0), (5, 5, 2, 0)]);
        assert_eq!(by_resource.winner(), Some(1));
        let by_buildings = res(&two, &[(5, 5, 2, 3), (5, 5, 2, 1)]);
        assert_eq!(by_buildings.winner(), Some(0));
    }

    #[test]
    fn surviving_longer_always_beats_dying_earlier() {
        let r = res(&[Some(90), Some(10), None], &[(0, 0, 0, 0), (0, 0, 0, 0), k()]);
        assert!(r.points_f64(0) > r.points_f64(1));
        assert!(r.points_f64(2) > r.points_f64(0));
    }

    #[test]
    fn metrics_aggregate() {
        let r = res(&[None, Some(5), Some(4), Some(3)], &vec![k(); 4]);
        let mut m = Metrics::default();
        m.record(&r, 0);
        m.record(&r, 3);
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
