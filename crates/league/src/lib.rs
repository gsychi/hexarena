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
use hexarena_sdk::{brains, roster, BotParams};
use rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
    /// The brain and its lineage, e.g. `Relay g3 ← g2` or `Relay g2 (new brain by agent tempo)`.
    #[serde(default)]
    pub name: String,
    /// The generations this genome was derived from; empty for a seed or a new brain.
    #[serde(default)]
    pub parents: Vec<String>,
    /// The champion it beat to be promoted.
    #[serde(default)]
    pub beat: Option<String>,
    /// Leagues written before `parents` and `beat` stored the beaten champion here.
    #[serde(default, skip_serializing)]
    parent: Option<String>,
    /// `seed`, `carryover`, `mutation`, `crossover`, or `agent:<name>` for inbox submissions.
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
    /// Generations the genome was derived from. Defaults to the champion when the brain
    /// matches the champion's, and to none (a new brain) otherwise.
    #[serde(default)]
    pub parents: Option<Vec<String>>,
}

/// A generation's display name: its brain, its id, and where it came from.
pub fn lineage_name(gen: u32, params: &BotParams, origin: &str, parents: &[String]) -> String {
    let brain = if params.brain.is_empty() { brains::DEFAULT } else { params.brain.as_str() };
    let mut chars = brain.chars();
    let brain: String = chars.next().map(|c| c.to_ascii_uppercase()).into_iter().chain(chars).collect();
    let from = match (parents, origin.strip_prefix("agent:")) {
        ([], Some(agent)) => format!("(new brain by agent {agent})"),
        ([], None) if origin == "carryover" => "(carried over)".into(),
        ([], None) => "(seed)".into(),
        (ps, Some(agent)) => format!("← {} (tuned by agent {agent})", ps.join(" × ")),
        (ps, None) => format!("← {}", ps.join(" × ")),
    };
    format!("{brain} g{gen} {from}")
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
    /// The opponents the stored benchmarks were played against.
    #[serde(default)]
    pub benchmark_roster: Vec<String>,
    /// Bots retired from the arena (`g1`, `raider`), left out of the sparring pool.
    #[serde(skip)]
    pub retired: BTreeSet<String>,
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

/// The classic CPUs every generation is benchmarked against. Fixed, so benchmarks stay
/// comparable across generations when stronger CPUs join the roster.
pub const BENCHMARK_ROSTER: &[&str] = &["expander", "homesteader", "raider", "warden"];

pub fn benchmark_entrants() -> Vec<Entrant> {
    roster_entrants()
        .into_iter()
        .filter(|e| BENCHMARK_ROSTER.iter().any(|id| e.id == format!("r-{id}")))
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
        if !Self::path(dir).exists() {
            return Self::seed(dir, BotParams::default(), "seed", "The hand-written Expander".into());
        }
        let mut old = Self::load(dir)?;
        if old.rules_version == RulesConfig::default().version {
            old.backfill_lineage();
            for g in &mut old.generations {
                brains::complete_knobs(&mut g.params)?;
            }
            if old.benchmark_roster != BENCHMARK_ROSTER {
                old.rebenchmark(dir)?;
            }
            old.save(dir)?;
            return Ok(old);
        }
        // Ratings and benchmarks don't carry across rules, so the old league is archived
        // and a new one starts from its champion.
        let archive = dir.join("archive").join(&old.rules_version);
        std::fs::create_dir_all(&archive).map_err(|e| e.to_string())?;
        for file in ["league.json", "games.jsonl"] {
            if dir.join(file).exists() {
                std::fs::rename(dir.join(file), archive.join(file)).map_err(|e| e.to_string())?;
            }
        }
        let c = old.champion();
        let note = format!("Champion {} of the rules {} league, carried over", c.id, old.rules_version);
        Self::seed(dir, c.params.clone(), "carryover", note)
    }

    fn seed(dir: &Path, mut params: BotParams, origin: &str, note: String) -> Result<League, String> {
        brains::complete_knobs(&mut params)?;
        let seed = Entrant { id: "g0".into(), params };
        let (bench, logs) = benchmark(&seed, &benchmark_entrants())?;
        let mut league = League {
            rules_version: RulesConfig::default().version,
            started_at: now_ms(),
            updated_at: now_ms(),
            gate_config: GateConfig::default(),
            generations: vec![Generation {
                gen: 0,
                name: lineage_name(0, &seed.params, origin, &[]),
                id: seed.id,
                parents: vec![],
                beat: None,
                parent: None,
                origin: origin.into(),
                note: Some(note),
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
            benchmark_roster: BENCHMARK_ROSTER.iter().map(|s| s.to_string()).collect(),
            retired: BTreeSet::new(),
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

    /// Fills `name`, `parents` and `beat` for generations saved before they existed.
    fn backfill_lineage(&mut self) {
        for i in 0..self.generations.len() {
            if !self.generations[i].name.is_empty() {
                continue;
            }
            let g = &self.generations[i];
            let beat = g.parent.clone();
            let parent_brain = beat
                .as_ref()
                .and_then(|p| self.generations.iter().find(|x| &x.id == p))
                .map(|x| x.params.brain.clone());
            let crossed = g.note.as_deref().and_then(|n| n.strip_prefix("with ")).map(String::from);
            let parents: Vec<String> = match (g.origin.as_str(), &beat) {
                ("mutation", Some(p)) => vec![p.clone()],
                ("crossover", Some(p)) => std::iter::once(p.clone()).chain(crossed).collect(),
                (_, Some(p)) if parent_brain.as_ref() == Some(&g.params.brain) => vec![p.clone()],
                _ => vec![],
            };
            let name = lineage_name(g.gen, &g.params, &g.origin, &parents);
            let g = &mut self.generations[i];
            g.parents = parents;
            g.beat = beat;
            g.name = name;
        }
    }

    /// Replays every generation's benchmark against [`BENCHMARK_ROSTER`].
    pub fn rebenchmark(&mut self, dir: &Path) -> Result<(), String> {
        let opponents = benchmark_entrants();
        for g in &mut self.generations {
            let entrant = Entrant { id: g.id.clone(), params: g.params.clone() };
            g.benchmark = benchmark(&entrant, &opponents)?.0;
        }
        self.benchmark_roster = BENCHMARK_ROSTER.iter().map(|s| s.to_string()).collect();
        self.save(dir)
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
    /// champions plus the roster, so a challenger can't win by exploiting the current
    /// champion alone. Retired bots are left out unless that would leave fewer than two.
    pub fn pool(&self) -> Vec<Entrant> {
        let all = self.full_pool();
        let active: Vec<Entrant> = all
            .iter()
            .filter(|e| !self.retired.contains(e.id.strip_prefix("r-").unwrap_or(&e.id)))
            .cloned()
            .collect();
        if active.len() >= 2 { active } else { all }
    }

    fn full_pool(&self) -> Vec<Entrant> {
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
        let (origin, note, mut params, parents) = match Self::take_submission(dir) {
            Some(s) => {
                let parents = s.parents.unwrap_or_else(|| {
                    if s.params.brain == champion.params.brain {
                        vec![champion.id.clone()]
                    } else {
                        vec![]
                    }
                });
                (s.origin, s.note, s.params, parents)
            }
            None if self.generations.len() > 2 && rng.chance(0.2) => {
                let other = &self.generations[rng.below(self.generations.len() - 1)];
                let child = mutate::crossover(&champion.params, &other.params, &mut rng);
                (
                    "crossover".into(),
                    Some(format!("with {}", other.id)),
                    child,
                    vec![champion.id.clone(), other.id.clone()],
                )
            }
            None => (
                "mutation".into(),
                None,
                mutate::mutate(&champion.params, &mut rng),
                vec![champion.id.clone()],
            ),
        };
        brains::complete_knobs(&mut params)?;
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
            let (bench, bench_logs) = benchmark(&entrant, &benchmark_entrants())?;
            let elo = self.elo.get(&challenger.id).copied().unwrap_or(START_ELO);
            self.elo.insert(id.clone(), elo);
            self.generations.push(Generation {
                gen,
                name: lineage_name(gen, &challenger.params, &origin, &parents),
                id,
                parents,
                beat: Some(champion.id.clone()),
                parent: None,
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
    fn names_spell_out_lineage() {
        let relay = BotParams { brain: "relay".into(), ..BotParams::default() };
        let base = BotParams::default();
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(lineage_name(0, &base, "carryover", &[]), "Expander g0 (carried over)");
        assert_eq!(lineage_name(2, &relay, "agent:tempo", &[]), "Relay g2 (new brain by agent tempo)");
        assert_eq!(lineage_name(3, &relay, "mutation", &ids(&["g2"])), "Relay g3 ← g2");
        assert_eq!(lineage_name(4, &relay, "crossover", &ids(&["g3", "g2"])), "Relay g4 ← g3 × g2");
        assert_eq!(lineage_name(5, &relay, "agent:tempo", &ids(&["g4"])), "Relay g5 ← g4 (tuned by agent tempo)");
    }

    #[test]
    fn brain_knobs_are_filled_in_so_they_can_mutate() {
        let mut p = BotParams { brain: "relay".into(), ..BotParams::default() };
        p.knobs.insert("patience".into(), 7);
        brains::complete_knobs(&mut p).unwrap();
        assert!(p.knobs.len() > 5);
        assert_eq!(p.knobs["patience"], 7);
        assert_eq!(p.knobs["margin_pct"], 150);
    }

    #[test]
    fn elo_rewards_better_placement_and_is_zero_sum_for_distinct_seats() {
        let mut elo = BTreeMap::new();
        let log = GameLog {
            seed: 0,
            ids: vec!["a".into(), "b".into(), "c".into()],
            avp: vec![1.0, 4.5, 8.0],
            at: 0,
            final_tick: 3000,
            closer: None,
        };
        apply_elo(&mut elo, &log);
        assert!(elo["a"] > START_ELO && elo["c"] < START_ELO);
        let total: f64 = elo.values().sum();
        assert!((total - 3.0 * START_ELO).abs() < 1e-9);
    }
}
