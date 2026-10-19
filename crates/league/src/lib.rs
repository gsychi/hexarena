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
use hexarena_sdk::process::Program;
use hexarena_sdk::{brains, roster, BotParams};
use rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const START_ELO: f64 = 1500.0;
pub const K: f64 = 32.0;
/// A game that ends before the cutoff moves ratings this many times as far.
pub const ENDED_WEIGHT: f64 = 2.0;
/// Challengers drawn per attempt before one that plays like the champion is gated anyway.
const SCREEN_TRIES: usize = 12;
/// Attempts kept in `league.json`; older ones are summarized by the counters.
const KEEP_ATTEMPTS: usize = 5000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
    pub gen: u32,
    pub id: String,
    /// The brain and its lineage, e.g. `Relay g3 ← g2` or `Relay g2 (new brain by agent tempo)`.
    #[serde(default)]
    pub name: String,
    /// What the name starts with instead of the brain, e.g. `Claude-Ridgeline` for a bot
    /// written by a Claude agent. Descendants inherit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
    /// The program that plays it when it was built outside this binary (an arena version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<Program>,
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
    /// The arena version this challenger was, when the arena sent it for a gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arena: Option<String>,
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
    /// The program that plays it, for bots built outside this binary.
    #[serde(default)]
    pub program: Option<Program>,
    /// The arena version this is, so its gate result can be shown next to it.
    #[serde(default)]
    pub arena: Option<String>,
    /// What the generation's name starts with, e.g. `GPT-Snowshoe`; defaults to the brain.
    #[serde(default)]
    pub label: Option<String>,
}

/// `Relay` for a genome playing the relay brain.
pub fn brain_label(params: &BotParams) -> String {
    let brain = if params.brain.is_empty() { brains::DEFAULT } else { params.brain.as_str() };
    let mut chars = brain.chars();
    chars.next().map(|c| c.to_ascii_uppercase()).into_iter().chain(chars).collect()
}

/// A generation's display name: its brain, its id, and where it came from.
pub fn lineage_name(gen: u32, params: &BotParams, origin: &str, parents: &[String]) -> String {
    lineage_name_as(&brain_label(params), gen, origin, parents)
}

/// [`lineage_name`] with `base` in place of the brain, e.g. `Claude-Thumper`.
pub fn lineage_name_as(brain: &str, gen: u32, origin: &str, parents: &[String]) -> String {
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
    /// Mutations dropped before a gate because they played exactly like the champion.
    #[serde(default)]
    pub screened_out: u64,
}

/// What decides which genes a bot reads: its program if it has one, else its brain.
pub fn family(params: &BotParams, program: Option<&Program>) -> String {
    match program {
        Some(p) => format!("program:{}", p.exe),
        None if params.brain.is_empty() => brains::DEFAULT.into(),
        None => params.brain.clone(),
    }
}

/// Knob defaults come from the built-in brains; a program's knobs are whatever it was
/// published with.
fn fill_knobs(params: &mut BotParams, program: Option<&Program>) -> Result<(), String> {
    match program {
        Some(_) => Ok(()),
        None => brains::complete_knobs(params),
    }
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
        .map(|s| Entrant::new(format!("r-{}", s.id), s.params))
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
        Self::requeue_interrupted(dir);
        if !Self::path(dir).exists() {
            return Self::seed(dir, BotParams::default(), None, 0, None, None, "seed", "The hand-written Expander".into());
        }
        let mut old = Self::load(dir)?;
        if old.rules_version == RulesConfig::default().version {
            old.backfill_lineage();
            for g in &mut old.generations {
                fill_knobs(&mut g.params, g.program.as_ref())?;
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
        // Numbering carries on from the old league, so a generation id always names one bot.
        let c = old.champion();
        let gen = c.gen + 1;
        let note = format!("Champion {} of the rules {} league, carried over", c.id, old.rules_version);
        let base = c.label.clone().unwrap_or_else(|| brain_label(&c.params));
        let name = format!("{base} g{gen} (carried over from {} {})", old.rules_version, c.id);
        Self::seed(dir, c.params.clone(), c.program.clone(), gen, Some(name), c.label.clone(), "carryover", note)
    }

    #[allow(clippy::too_many_arguments)]
    fn seed(
        dir: &Path,
        mut params: BotParams,
        program: Option<Program>,
        gen: u32,
        name: Option<String>,
        label: Option<String>,
        origin: &str,
        note: String,
    ) -> Result<League, String> {
        fill_knobs(&mut params, program.as_ref())?;
        let seed = Entrant { id: format!("g{gen}"), params, program };
        let (bench, logs) = benchmark(&seed, &benchmark_entrants())?;
        let mut league = League {
            rules_version: RulesConfig::default().version,
            started_at: now_ms(),
            updated_at: now_ms(),
            gate_config: GateConfig::default(),
            generations: vec![Generation {
                gen,
                name: name.unwrap_or_else(|| lineage_name(gen, &seed.params, origin, &[])),
                label,
                id: seed.id,
                parents: vec![],
                beat: None,
                parent: None,
                origin: origin.into(),
                note: Some(note),
                created_at: now_ms(),
                params: seed.params,
                program: seed.program,
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
            screened_out: 0,
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

    /// Draws challengers from `make` until one plays differently from the champion, up to
    /// [`SCREEN_TRIES`] draws. Most small nudges change nothing at all, and a full gate
    /// would spend 16 games finding that out; the screen spends 4.
    fn screened(
        &mut self,
        champion: &Entrant,
        n: u64,
        rng: &mut Rng,
        mut make: impl FnMut(&mut Rng) -> BotParams,
    ) -> Result<BotParams, String> {
        let pool = self.pool();
        let mut child = make(rng);
        for _ in 1..SCREEN_TRIES {
            let mut c = Entrant { id: "screen".into(), params: child.clone(), program: champion.program.clone() };
            fill_knobs(&mut c.params, c.program.as_ref())?;
            if !plays_like(&c, champion, &pool, 20_000_000 + n * 16)? {
                break;
            }
            self.screened_out += 1;
            child = make(rng);
        }
        Ok(child)
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
            let name = match &g.label {
                Some(base) => lineage_name_as(base, g.gen, &g.origin, &parents),
                None => lineage_name(g.gen, &g.params, &g.origin, &parents),
            };
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
            let entrant = Entrant { id: g.id.clone(), params: g.params.clone(), program: g.program.clone() };
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
            program: c.program.clone(),
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
                program: g.program.clone(),
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

    /// The next inbox submission, moved to `inbox/active/` until its gate is recorded.
    /// Agents take turns: the oldest submission from an origin other than the last
    /// gated one goes first, so one agent shipping many versions can't starve the rest.
    fn take_submission(&self, dir: &Path) -> Option<(Submission, PathBuf)> {
        let inbox = dir.join("inbox");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&inbox)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        let done = inbox.join("done");
        let active = inbox.join("active");
        let _ = std::fs::create_dir_all(&done);
        let _ = std::fs::create_dir_all(&active);
        let mut parsed = vec![];
        for path in files {
            match read_json::<Submission>(&path) {
                Ok(s) => parsed.push((s, path)),
                Err(e) => {
                    eprintln!("skipping bad submission: {e}");
                    let _ = std::fs::rename(&path, done.join(path.file_name().unwrap()));
                }
            }
        }
        let last = self.attempts.iter().rev().find(|a| a.origin.starts_with("agent:")).map(|a| a.origin.clone());
        let pick = parsed.iter().position(|(s, _)| Some(&s.origin) != last.as_ref()).unwrap_or(0);
        if parsed.is_empty() {
            return None;
        }
        let (sub, path) = parsed.swap_remove(pick);
        let moved = active.join(path.file_name().unwrap());
        std::fs::rename(&path, &moved).ok()?;
        Some((sub, moved))
    }

    /// Puts submissions whose gate was interrupted (the league stopped mid-gate) back in
    /// the inbox.
    fn requeue_interrupted(dir: &Path) {
        let inbox = dir.join("inbox");
        for e in std::fs::read_dir(inbox.join("active")).into_iter().flatten().flatten() {
            let _ = std::fs::rename(e.path(), inbox.join(e.file_name()));
        }
    }

    /// Runs one challenger through the gate and promotes it if it wins.
    pub fn attempt(&mut self, dir: &Path) -> Result<Attempt, String> {
        let n = self.attempts_total + 1;
        let mut rng = Rng::new(self.started_at ^ n.wrapping_mul(0x9E37_79B9));
        let champion = self.champion_entrant();
        let champion_gen = self.champion().gen;
        let fam = family(&champion.params, champion.program.as_ref());
        // Crossover partners must play the same way as the champion for their genes to mean the same thing.
        let partners: Vec<(String, BotParams)> = self.generations[..self.generations.len() - 1]
            .iter()
            .filter(|g| family(&g.params, g.program.as_ref()) == fam)
            .map(|g| (g.id.clone(), g.params.clone()))
            .collect();
        let champion_label = self.champion().label.clone();
        let taken = self.take_submission(dir);
        let active = taken.as_ref().map(|(_, p)| p.clone());
        let (origin, note, mut params, parents, program, arena, label) = match taken.map(|(s, _)| s) {
            Some(s) => {
                let parents = s.parents.unwrap_or_else(|| {
                    if family(&s.params, s.program.as_ref()) == fam {
                        vec![champion.id.clone()]
                    } else {
                        vec![]
                    }
                });
                (s.origin, s.note, s.params, parents, s.program, s.arena, s.label)
            }
            None if partners.len() >= 2 && rng.chance(0.2) => {
                let (other, other_params) = partners[rng.below(partners.len())].clone();
                let child = self.screened(&champion, n, &mut rng, |rng| {
                    mutate::crossover(&champion.params, &other_params, rng)
                })?;
                (
                    "crossover".into(),
                    Some(format!("with {other}")),
                    child,
                    vec![champion.id.clone(), other],
                    champion.program.clone(),
                    None,
                    champion_label.clone(),
                )
            }
            None => {
                let child = self.screened(&champion, n, &mut rng, |rng| mutate::mutate(&champion.params, rng))?;
                ("mutation".into(), None, child, vec![champion.id.clone()], champion.program.clone(), None, champion_label.clone())
            }
        };
        fill_knobs(&mut params, program.as_ref())?;
        let challenger = Entrant {
            id: format!("c{n}"),
            params,
            program,
        };
        let seed_base = 10_000_000 + n * 1000;
        let (result, logs) = match gate(&challenger, &champion, &self.pool(), seed_base, &self.gate_config) {
            Ok(r) => r,
            Err(e) => {
                // A submission that can't play is set aside instead of retried forever.
                if let Some(p) = &active {
                    let failed = dir.join("inbox").join("failed");
                    let _ = std::fs::create_dir_all(&failed);
                    let _ = std::fs::rename(p, failed.join(p.file_name().unwrap()));
                }
                return Err(format!("{origin} challenger couldn't play: {e}"));
            }
        };
        self.play_secs += result.secs;
        self.record_games(dir, &logs)?;
        let changes = if family(&challenger.params, challenger.program.as_ref()) == fam {
            mutate::diff(&champion.params, &challenger.params)
        } else {
            let brain = |p: &BotParams| if p.brain.is_empty() { brains::DEFAULT.to_string() } else { p.brain.clone() };
            let built = if challenger.program.is_some() { " (own build)" } else { "" };
            vec![format!("brain {}→{}{built}", brain(&champion.params), brain(&challenger.params))]
        };
        let mut became = None;
        if result.promoted {
            let gen = champion_gen + 1;
            let id = format!("g{gen}");
            let entrant = Entrant {
                id: id.clone(),
                params: challenger.params.clone(),
                program: challenger.program.clone(),
            };
            let (bench, bench_logs) = benchmark(&entrant, &benchmark_entrants())?;
            let elo = self.elo.get(&challenger.id).copied().unwrap_or(START_ELO);
            self.elo.insert(id.clone(), elo);
            self.generations.push(Generation {
                gen,
                name: match &label {
                    Some(base) => lineage_name_as(base, gen, &origin, &parents),
                    None => lineage_name(gen, &challenger.params, &origin, &parents),
                },
                label: label.clone(),
                id,
                parents,
                beat: Some(champion.id.clone()),
                parent: None,
                origin: origin.clone(),
                note: note.clone(),
                created_at: now_ms(),
                params: challenger.params.clone(),
                program: challenger.program.clone(),
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
            arena,
        };
        self.attempts.push(attempt.clone());
        if self.attempts.len() > KEEP_ATTEMPTS {
            self.attempts.drain(..self.attempts.len() - KEEP_ATTEMPTS);
        }
        self.attempts_total = n;
        self.save(dir)?;
        if let Some(p) = active {
            let _ = std::fs::rename(&p, dir.join("inbox").join("done").join(p.file_name().unwrap()));
        }
        Ok(attempt)
    }
}

/// True when `a` in three seats of a lobby leaves exactly the same placements and
/// ending as `b` in those seats, on two seeds: the change didn't alter a single move
/// that mattered.
fn plays_like(a: &Entrant, b: &Entrant, pool: &[Entrant], seed: u64) -> Result<bool, String> {
    let lobbies: Vec<(u64, Vec<Entrant>)> = (0..2u64)
        .flat_map(|k| {
            let mut rng = Rng::new(seed ^ k ^ 0x5C2E);
            let mut seats = vec![a.clone(); 3];
            seats.extend(vec![b.clone(); 3]);
            for _ in 0..eval::LOBBY - 6 {
                seats.push(if pool.is_empty() { b.clone() } else { pool[rng.below(pool.len())].clone() });
            }
            rng.shuffle(&mut seats);
            let swapped: Vec<Entrant> = seats.iter().map(|e| if e.id == a.id { b.clone() } else { e.clone() }).collect();
            [(seed + k, seats), (seed + k, swapped)]
        })
        .collect();
    let logs = eval::play_all(&lobbies)?;
    Ok(logs.chunks(2).all(|p| p[0].avp == p[1].avp && p[0].final_tick == p[1].final_tick))
}

/// Multiplayer Elo: every pair of seats with different identities is a head-to-head
/// result (lower 8-player placement wins), scaled by K/(N-1); an identity with several
/// seats moves by the average of its seats. Games that ended before the cutoff count
/// [`ENDED_WEIGHT`] times.
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
        let k = if log.closer.is_some() { K * ENDED_WEIGHT } else { K };
        e.0 += k / (n - 1) as f64 * d;
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
    fn screen_spots_changes_that_change_nothing() {
        let champ = Entrant::new("h", BotParams::default());
        let pool = benchmark_entrants();
        let same = Entrant::new("c", BotParams::default());
        assert!(plays_like(&same, &champ, &pool, 7).unwrap());
        let timid = Entrant::new("c", BotParams { land_value: 0, attack_margin_pct: 260, ..BotParams::default() });
        assert!(!plays_like(&timid, &champ, &pool, 7).unwrap());
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
