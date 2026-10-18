//! The Hexarena league: an AlphaGo-style improvement loop for CPU genomes.
//!
//! Each attempt pits a challenger (a mutation of the champion, a crossover with an
//! earlier generation, or a genome submitted to the inbox) against the current champion
//! in mirrored 8-player lobbies. A challenger that is significantly better on the
//! 8-player placement scale becomes the next generation. Every generation is also
//! benchmarked against the original hand-written roster on fixed seeds, and all league
//! games feed a multiplayer Elo, so progress over time is visible.

pub mod eval;
pub mod mutate;
pub mod rng;

use eval::{benchmark, gate, Benchmark, Entrant, GameLog, GateConfig, GateResult};
use hexarena_protocol::RulesConfig;
use hexarena_sdk::{roster, BotParams};
use rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const START_ELO: f64 = 1500.0;
pub const K: f64 = 32.0;
/// Attempts kept in `league.json`; older ones are summarized by the counters.
const KEEP_ATTEMPTS: usize = 5000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
    pub gen: u32,
    pub id: String,
    pub parent: Option<String>,
    /// `seed`, `mutation`, `crossover`, or `agent:<name>` for inbox submissions.
    pub origin: String,
    pub note: Option<String>,
    pub created_at: u64,
    pub params: BotParams,
    /// Gene changes relative to the parent.
    pub changes: Vec<String>,
    pub gate: Option<GateResult>,
    pub benchmark: Benchmark,
    pub elo: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub n: u64,
    pub at: u64,
    pub id: String,
    pub origin: String,
    pub note: Option<String>,
    pub parent: String,
    pub changes: Vec<String>,
    pub gate: GateResult,
    /// The generation this attempt became, if promoted.
    pub became: Option<u32>,
}

/// A genome dropped into `inbox/` to be gated ahead of random mutations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Submission {
    pub origin: String,
    #[serde(default)]
    pub note: Option<String>,
    pub params: BotParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct League {
    pub rules_version: String,
    pub started_at: u64,
    pub updated_at: u64,
    pub gate_config: GateConfig,
    pub generations: Vec<Generation>,
    pub attempts: Vec<Attempt>,
    pub attempts_total: u64,
    pub games_total: u64,
    pub play_secs: f64,
    /// Current Elo of every identity that has played (generations, roster, challengers).
    pub elo: BTreeMap<String, f64>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The hand-written roster as league entrants (`r-expander`, ...).
pub fn roster_entrants() -> Vec<Entrant> {
    roster()
        .into_iter()
        .map(|s| Entrant {
            id: format!("r-{}", s.id),
            params: s.params,
        })
        .collect()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

impl League {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join("league.json")
    }

    pub fn load(dir: &Path) -> Result<League, String> {
        read_json(&Self::path(dir))
    }

    /// Loads the league in `dir`, or seeds a new one whose generation 0 is the
    /// hand-written Expander.
    pub fn open(dir: &Path) -> Result<League, String> {
        std::fs::create_dir_all(dir.join("inbox")).map_err(|e| e.to_string())?;
        if Self::path(dir).exists() {
            return Self::load(dir);
        }
        let seed = Entrant {
            id: "g0".into(),
            params: BotParams::default(),
        };
        let (bench, logs) = benchmark(&seed, &roster_entrants())?;
        let mut league = League {
            rules_version: RulesConfig::default().version,
            started_at: now_ms(),
            updated_at: now_ms(),
            gate_config: GateConfig::default(),
            generations: vec![Generation {
                gen: 0,
                id: seed.id,
                parent: None,
                origin: "seed".into(),
                note: Some("The hand-written Expander".into()),
                created_at: now_ms(),
                params: seed.params,
                changes: vec![],
                gate: None,
                benchmark: bench,
                elo: START_ELO,
            }],
            attempts: vec![],
            attempts_total: 0,
            games_total: 0,
            play_secs: 0.0,
            elo: BTreeMap::new(),
        };
        league.record_games(dir, &logs)?;
        league.save(dir)?;
        Ok(league)
    }

    pub fn save(&mut self, dir: &Path) -> Result<(), String> {
        self.updated_at = now_ms();
        let tmp = dir.join("league.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, Self::path(dir)).map_err(|e| e.to_string())
    }

    pub fn champion(&self) -> &Generation {
        self.generations.last().expect("league has generation 0")
    }

    pub fn champion_entrant(&self) -> Entrant {
        let c = self.champion();
        Entrant {
            id: c.id.clone(),
            params: c.params.clone(),
        }
    }

    /// Opponents for the two open seats of a gate lobby: up to the 8 most recent earlier
    /// champions plus the original roster, so a challenger can't win by exploiting the
    /// current champion alone.
    pub fn pool(&self) -> Vec<Entrant> {
        let earlier = self.generations.len().saturating_sub(1);
        let mut pool: Vec<Entrant> = self.generations[earlier.saturating_sub(8)..earlier]
            .iter()
            .map(|g| Entrant {
                id: g.id.clone(),
                params: g.params.clone(),
            })
            .collect();
        pool.extend(roster_entrants());
        pool
    }

    fn record_games(&mut self, dir: &Path, logs: &[GameLog]) -> Result<(), String> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("games.jsonl"))
            .map_err(|e| e.to_string())?;
        for log in logs {
            writeln!(f, "{}", serde_json::to_string(log).unwrap()).map_err(|e| e.to_string())?;
            apply_elo(&mut self.elo, log);
        }
        self.games_total += logs.len() as u64;
        for g in &mut self.generations {
            g.elo = self.elo.get(&g.id).copied().unwrap_or(START_ELO);
        }
        Ok(())
    }

    /// The oldest inbox submission, removed from the inbox.
    fn take_submission(dir: &Path) -> Option<Submission> {
        let inbox = dir.join("inbox");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&inbox)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for path in files {
            let parsed: Result<Submission, String> = read_json(&path);
            let done = inbox.join("done");
            let _ = std::fs::create_dir_all(&done);
            let _ = std::fs::rename(&path, done.join(path.file_name().unwrap()));
            match parsed {
                Ok(s) => return Some(s),
                Err(e) => eprintln!("skipping bad submission: {e}"),
            }
        }
        None
    }

    /// Runs one challenger through the gate and promotes it if it wins.
    pub fn attempt(&mut self, dir: &Path) -> Result<Attempt, String> {
        let n = self.attempts_total + 1;
        let mut rng = Rng::new(self.started_at ^ n.wrapping_mul(0x9E37_79B9));
        let champion = self.champion_entrant();
        let (origin, note, params) = match Self::take_submission(dir) {
            Some(s) => (s.origin, s.note, s.params),
            None if self.generations.len() > 2 && rng.chance(0.2) => {
                let other = &self.generations[rng.below(self.generations.len() - 1)];
                let child = mutate::crossover(&champion.params, &other.params, &mut rng);
                (
                    "crossover".into(),
                    Some(format!("with {}", other.id)),
                    child,
                )
            }
            None => ("mutation".into(), None, mutate::mutate(&champion.params, &mut rng)),
        };
        let challenger = Entrant {
            id: format!("c{n}"),
            params,
        };
        let seed_base = 10_000_000 + n * 1000;
        let (result, logs) = gate(&challenger, &champion, &self.pool(), seed_base, &self.gate_config)?;
        self.play_secs += result.secs;
        self.record_games(dir, &logs)?;
        let changes = mutate::diff(&champion.params, &challenger.params);
        let mut became = None;
        if result.promoted {
            let gen = self.generations.len() as u32;
            let id = format!("g{gen}");
            let entrant = Entrant {
                id: id.clone(),
                params: challenger.params.clone(),
            };
            let (bench, bench_logs) = benchmark(&entrant, &roster_entrants())?;
            let elo = self.elo.get(&challenger.id).copied().unwrap_or(START_ELO);
            self.elo.insert(id.clone(), elo);
            self.generations.push(Generation {
                gen,
                id,
                parent: Some(champion.id.clone()),
                origin: origin.clone(),
                note: note.clone(),
                created_at: now_ms(),
                params: challenger.params.clone(),
                changes: changes.clone(),
                gate: Some(result.clone()),
                benchmark: bench,
                elo,
            });
            self.record_games(dir, &bench_logs)?;
            became = Some(gen);
        }
        let attempt = Attempt {
            n,
            at: now_ms(),
            id: challenger.id,
            origin,
            note,
            parent: champion.id,
            changes,
            gate: result,
            became,
        };
        self.attempts.push(attempt.clone());
        if self.attempts.len() > KEEP_ATTEMPTS {
            self.attempts.drain(..self.attempts.len() - KEEP_ATTEMPTS);
        }
        self.attempts_total = n;
        self.save(dir)?;
        Ok(attempt)
    }
}

/// Multiplayer Elo: every pair of seats with different identities is a head-to-head
/// result (lower 8-player placement wins), scaled by K/(N-1); an identity with several
/// seats moves by the average of its seats.
pub fn apply_elo(elo: &mut BTreeMap<String, f64>, log: &GameLog) {
    let n = log.ids.len();
    if n < 2 {
        return;
    }
    let rating = |id: &str, elo: &BTreeMap<String, f64>| elo.get(id).copied().unwrap_or(START_ELO);
    let mut delta: BTreeMap<&str, (f64, u32)> = BTreeMap::new();
    for i in 0..n {
        let ri = rating(&log.ids[i], elo);
        let mut d = 0.0;
        for j in 0..n {
            if log.ids[i] == log.ids[j] {
                continue;
            }
            let rj = rating(&log.ids[j], elo);
            let expected = 1.0 / (1.0 + 10f64.powf((rj - ri) / 400.0));
            let actual = match log.avp[i].partial_cmp(&log.avp[j]) {
                Some(std::cmp::Ordering::Less) => 1.0,
                Some(std::cmp::Ordering::Greater) => 0.0,
                _ => 0.5,
            };
            d += actual - expected;
        }
        let e = delta.entry(&log.ids[i]).or_insert((0.0, 0));
        e.0 += K / (n - 1) as f64 * d;
        e.1 += 1;
    }
    for (id, (sum, seats)) in delta {
        *elo.entry(id.to_string()).or_insert(START_ELO) += sum / seats as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elo_rewards_better_placement_and_is_zero_sum_for_distinct_seats() {
        let mut elo = BTreeMap::new();
        let log = GameLog {
            seed: 0,
            ids: vec!["a".into(), "b".into(), "c".into()],
            avp: vec![1.0, 4.5, 8.0],
        };
        apply_elo(&mut elo, &log);
        assert!(elo["a"] > START_ELO && elo["c"] < START_ELO);
        let total: f64 = elo.values().sum();
        assert!((total - 3.0 * START_ELO).abs() < 1e-9);
    }
}
