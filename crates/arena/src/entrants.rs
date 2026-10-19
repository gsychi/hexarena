//! Arena entrants: bot versions that people or agents publish to try them out in the
//! self-playing arena. Each one is a genome plus, usually, a frozen copy of the program
//! that plays it, so a version keeps playing the same way after its author's code moves
//! on. Versions are immutable: publish a new id to try a change.

use crate::{play_result, seat_names, settings};
use hexarena_engine::State;
use hexarena_sdk::process::{probe, Program};
use hexarena_sdk::{roster, BotParams, Style};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entrant {
    pub id: String,
    pub name: String,
    /// Who published it, e.g. `agent:predator` or `gordon`.
    pub author: String,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: u64,
    /// Retired entrants keep their results but are no longer scheduled.
    #[serde(default)]
    pub retired: bool,
    #[serde(default)]
    pub retired_reason: Option<String>,
    pub params: BotParams,
    /// The frozen program that plays it; `None` uses the brains compiled into the server.
    #[serde(default)]
    pub program: Option<Program>,
}

impl Entrant {
    pub fn style(&self) -> Style {
        Style {
            id: self.id.clone(),
            name: self.name.clone(),
            blurb: self.note.clone().unwrap_or_else(|| format!("Published by {}", self.author)),
            params: self.params.clone(),
            program: self.program.clone(),
        }
    }
}

fn entrant_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("entrants").join(format!("{id}.json"))
}

/// Every published entrant, oldest first.
pub fn load_all(dir: &Path) -> Vec<Entrant> {
    let mut out: Vec<Entrant> = std::fs::read_dir(dir.join("entrants"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a: &Entrant, b: &Entrant| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    out
}

/// Ids are lowercase letters, digits and dashes, and can't shadow a roster CPU or a
/// league generation (`g3`, `champion`).
pub fn check_id(id: &str) -> Result<(), String> {
    let shape = (2..=40).contains(&id.len())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-');
    if !shape {
        return Err(format!("bad id '{id}': use 2-40 lowercase letters, digits and dashes"));
    }
    let generation = id.strip_prefix('g').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if id == "champion" || generation || roster().iter().any(|s| s.id == id) {
        return Err(format!("'{id}' is reserved for a roster CPU or league generation"));
    }
    Ok(())
}

/// Publishes `entrant`. With `exe`, that program is copied into the arena so the version
/// stays frozen, and plays the seat through `exe bot`. The entrant must survive a short
/// trial game before it is listed.
pub fn publish(dir: &Path, mut entrant: Entrant, exe: Option<&Path>) -> Result<Entrant, String> {
    check_id(&entrant.id)?;
    let path = entrant_path(dir, &entrant.id);
    if path.exists() {
        return Err(format!("'{}' is already published; versions are immutable, so pick a new id", entrant.id));
    }
    std::fs::create_dir_all(dir.join("entrants")).map_err(|e| e.to_string())?;
    if let Some(exe) = exe {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        let frozen = std::path::absolute(bin.join(&entrant.id)).map_err(|e| e.to_string())?;
        std::fs::copy(exe, &frozen).map_err(|e| format!("{}: {e}", exe.display()))?;
        entrant.program = Some(Program { exe: frozen.to_string_lossy().into(), args: vec!["bot".into()] });
    }
    let trial = [entrant.style(), roster().remove(0)];
    if let Some(program) = &entrant.program {
        let st = State::new(settings(2, 1), seat_names(&trial))?;
        probe(program, &trial[0], &st.view(0))?;
    }
    play_result(1, &trial, 300).map_err(|e| format!("trial game failed: {e}"))?;
    std::fs::write(&path, serde_json::to_vec_pretty(&entrant).unwrap()).map_err(|e| e.to_string())?;
    Ok(entrant)
}

pub fn retire(dir: &Path, id: &str, reason: Option<String>) -> Result<Entrant, String> {
    let path = entrant_path(dir, id);
    let bytes = std::fs::read(&path).map_err(|_| format!("no entrant '{id}'"))?;
    let mut e: Entrant = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    e.retired = true;
    e.retired_reason = reason;
    std::fs::write(&path, serde_json::to_vec_pretty(&e).unwrap()).map_err(|e| e.to_string())?;
    Ok(e)
}

/// Why and when a roster CPU or league generation left the arena.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Retirement {
    pub reason: String,
    pub at: u64,
}

/// Retired bots that aren't arena entrants (roster CPUs, league generations), from
/// `retired.json`.
pub fn retired_others(dir: &Path) -> BTreeMap<String, Retirement> {
    std::fs::read(dir.join("retired.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn retire_other(dir: &Path, id: &str, reason: String, at: u64) -> Result<(), String> {
    let mut all = retired_others(dir);
    all.insert(id.to_string(), Retirement { reason, at });
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("retired.json"), serde_json::to_vec_pretty(&all).unwrap()).map_err(|e| e.to_string())
}

/// Every retired id: entrants and other bots.
pub fn retired_ids(dir: &Path) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = retired_others(dir).into_keys().collect();
    ids.extend(load_all(dir).into_iter().filter(|e| e.retired).map(|e| e.id));
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_cannot_shadow_roster_or_generations() {
        assert!(check_id("predator-v2").is_ok());
        for bad in ["predator", "champion", "g12", "Bad", "-x", "a"] {
            assert!(check_id(bad).is_err(), "{bad}");
        }
        assert!(check_id("g12-tweak").is_ok());
    }
}
