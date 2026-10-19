//! League generations as playable CPUs, and the league snapshot for the web page.

use hexarena_arena::entrants::{load_all, retired_others, Entrant, Retirement};
use std::collections::{BTreeMap, BTreeSet};
use hexarena_league::eval::GameLog;
use hexarena_league::League;
use hexarena_sdk::{roster, style, Style};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Attempts sent to the browser; the chart only needs the recent history.
const SNAPSHOT_ATTEMPTS: usize = 1500;

pub fn load(dir: &Path) -> Option<League> {
    League::load(dir).ok()
}

/// Everything a seat id can name: roster CPUs, league generations and arena entrants.
pub struct Catalog {
    pub league: Option<League>,
    pub entrants: Vec<Entrant>,
    /// Retired roster CPUs and league generations.
    pub others_retired: BTreeMap<String, Retirement>,
    /// Every retired id, entrants included.
    pub retired: BTreeSet<String>,
}

impl Catalog {
    pub fn load(league_dir: &Path, arena_dir: &Path) -> Catalog {
        let entrants = load_all(arena_dir);
        let others_retired = retired_others(arena_dir);
        let mut retired: BTreeSet<String> = others_retired.keys().cloned().collect();
        retired.extend(entrants.iter().filter(|e| e.retired).map(|e| e.id.clone()));
        Catalog { league: load(league_dir), entrants, others_retired, retired }
    }

    /// Why each retired bot was retired, by id.
    pub fn retired_reasons(&self) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> =
            self.others_retired.iter().map(|(id, r)| (id.clone(), r.reason.clone())).collect();
        for e in self.entrants.iter().filter(|e| e.retired) {
            out.insert(e.id.clone(), e.retired_reason.clone().unwrap_or_else(|| "retired".into()));
        }
        out
    }

    /// Roster CPUs and league generations that still play in the arena.
    pub fn active_field(&self) -> Vec<Style> {
        let champion = self.league.as_ref().map(|l| l.champion().id.clone());
        let gens = self.league.iter().flat_map(|l| l.generations.iter().map(|g| g.id.clone()));
        roster()
            .into_iter()
            .map(|s| s.id)
            .chain(gens)
            .filter(|id| !self.retired.contains(id) && Some(id) != champion.as_ref())
            .filter_map(|id| self.resolve(&id))
            .collect()
    }

    /// A roster style, a league generation (`g12`), `champion` for the latest generation,
    /// or an arena entrant (retired ones too, so old games keep their names).
    /// Generations keep their own id so their results never blur together.
    pub fn resolve(&self, id: &str) -> Option<Style> {
        if let Some(s) = style(id) {
            return Some(s);
        }
        if let Some(e) = self.entrants.iter().find(|e| e.id == id) {
            return Some(e.style());
        }
        let league = self.league.as_ref()?;
        let g = if id == "champion" {
            league.champion()
        } else {
            league.generations.iter().find(|g| g.id == id)?
        };
        Some(Style {
            id: g.id.clone(),
            name: if g.name.is_empty() { format!("League {}", g.id) } else { g.name.clone() },
            blurb: match (g.parents.as_slice(), &g.beat) {
                ([], Some(beat)) => format!("Generation {}, a new brain that beat champion {beat}.", g.gen),
                ([], None) => format!("Generation {}, the league's starting genome.", g.gen),
                (ps, Some(beat)) => format!("Generation {}, bred from {}; beat champion {beat}.", g.gen, ps.join(" × ")),
                (ps, None) => format!("Generation {}, bred from {}.", g.gen, ps.join(" × ")),
            },
            params: g.params.clone(),
            program: g.program.clone(),
        })
    }

    pub fn display_name(&self, id: &str) -> String {
        self.resolve(id).map(|s| s.name).unwrap_or_else(|| id.to_string())
    }

    pub fn active_entrants(&self) -> impl Iterator<Item = &Entrant> {
        self.entrants.iter().filter(|e| !e.retired)
    }

    /// Every playable bot, grouped by `kind`: roster CPUs, `champion` plus every league
    /// generation (newest first), and every arena version (newest first). Retired bots are
    /// still playable and carry `retired` with the reason.
    pub fn bots(&self) -> Vec<serde_json::Value> {
        let reasons = self.retired_reasons();
        let retired = |id: &str| serde_json::json!(reasons.get(id));
        let mut out: Vec<serde_json::Value> = roster()
            .iter()
            .map(|s| serde_json::json!({ "id": s.id, "name": s.name, "blurb": s.blurb, "kind": "roster", "retired": retired(&s.id) }))
            .collect();
        if let Some(l) = &self.league {
            let c = l.champion();
            out.push(serde_json::json!({
                "id": "champion",
                "name": format!("League champion: {}", if c.name.is_empty() { &c.id } else { &c.name }),
                "blurb": format!("Always the latest champion, currently generation {}.", c.gen),
                "kind": "league",
                "retired": null,
            }));
            for g in l.generations.iter().rev() {
                let s = self.resolve(&g.id).expect("generation resolves");
                out.push(serde_json::json!({ "id": g.id, "name": s.name, "blurb": s.blurb, "kind": "league", "retired": retired(&g.id) }));
            }
        }
        let mut entrants: Vec<&Entrant> = self.entrants.iter().collect();
        entrants.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.extend(entrants.into_iter().map(|e| {
            serde_json::json!({
                "id": e.id,
                "name": e.name,
                "blurb": e.style().blurb,
                "kind": "entrant",
                "author": e.author,
                "brain": if e.params.brain.is_empty() { "expander" } else { e.params.brain.as_str() },
                "own_build": e.program.is_some(),
                "created_at": e.created_at,
                "retired": retired(&e.id),
            })
        }));
        out
    }
}

/// A league game reduced to the seats that count in the standings.
pub struct RatedGame {
    pub at: u64,
    pub players: usize,
    /// (index into [`LeagueGames::names`], 8-player-scale place).
    pub seats: Vec<(usize, f64)>,
}

/// League games from `games.jsonl`, read incrementally as the league appends to it.
/// Only generations (`g3`) and roster CPUs (`r-raider`, rated as `raider`) are kept;
/// one-off challengers are dropped from their games.
#[derive(Default)]
pub struct LeagueGames {
    offset: u64,
    pub names: Vec<String>,
    index: HashMap<String, Option<usize>>,
    pub games: Vec<RatedGame>,
}

fn rated_id(id: &str) -> Option<String> {
    if let Some(r) = id.strip_prefix("r-") {
        return style(r).map(|s| s.id);
    }
    let n = id.strip_prefix('g')?;
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
}

impl LeagueGames {
    fn intern(&mut self, id: &str) -> Option<usize> {
        if let Some(i) = self.index.get(id) {
            return *i;
        }
        let i = rated_id(id).map(|name| {
            if let Some(at) = self.names.iter().position(|n| *n == name) {
                return at;
            }
            self.names.push(name);
            self.names.len() - 1
        });
        self.index.insert(id.to_string(), i);
        i
    }

    /// Reads whatever the league appended since the last call. A shorter file means
    /// the league was reset, so everything is reread.
    pub fn refresh(&mut self, dir: &Path) {
        let Ok(mut f) = std::fs::File::open(dir.join("games.jsonl")) else {
            *self = LeagueGames::default();
            return;
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            *self = LeagueGames::default();
        }
        if len == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buf = Vec::with_capacity((len - self.offset) as usize);
        if f.read_to_end(&mut buf).is_err() {
            return;
        }
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return };
        for line in buf[..end].split(|&b| b == b'\n') {
            let Ok(log) = serde_json::from_slice::<GameLog>(line) else { continue };
            let players = log.ids.len();
            let seats: Vec<(usize, f64)> = log
                .ids
                .iter()
                .zip(&log.avp)
                .filter_map(|(id, &avp)| Some((self.intern(id)?, avp)))
                .collect();
            if seats.len() >= 2 {
                self.games.push(RatedGame { at: log.at, players, seats });
            }
        }
        self.offset += end as u64 + 1;
    }
}

pub fn snapshot(mut league: League) -> serde_json::Value {
    let keep = league.attempts.len().saturating_sub(SNAPSHOT_ATTEMPTS);
    league.attempts.drain(..keep);
    serde_json::to_value(league).unwrap()
}
