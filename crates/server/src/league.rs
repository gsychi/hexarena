//! League generations as playable CPUs, and the league snapshot for the web page.

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

/// A roster style, a league generation (`g12`), or `champion` for the latest
/// generation. Generations keep their own id so their results never blur together.
pub fn resolve(id: &str, league: Option<&League>) -> Option<Style> {
    if let Some(s) = style(id) {
        return Some(s);
    }
    let league = league?;
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
    })
}

pub fn display_name(id: &str, league: Option<&League>) -> String {
    resolve(id, league).map(|s| s.name).unwrap_or_else(|| id.to_string())
}

pub fn bots(league: Option<&League>) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = roster()
        .iter()
        .map(|s| serde_json::json!({ "id": s.id, "name": s.name, "blurb": s.blurb }))
        .collect();
    if let Some(l) = league {
        let c = l.champion();
        out.push(serde_json::json!({
            "id": "champion",
            "name": format!("League champion: {}", if c.name.is_empty() { &c.id } else { &c.name }),
            "blurb": format!("The strongest genome the league has evolved so far: generation {}.", c.gen),
        }));
    }
    out
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
