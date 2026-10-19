//! Arena entrants: bot versions that people or agents publish to try them out in the
//! self-playing arena. Each one is a genome plus, usually, a frozen copy of the program
//! that plays it, so a version keeps playing the same way after its author's code moves
//! on. Versions are immutable: publish a new id to try a change.

use crate::{play_result, seat_names, settings};
use hexarena_engine::State;
use hexarena_protocol::RulesConfig;
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
    /// The bots it takes its policy from, as ids (arena versions or roster CPUs); empty
    /// for a design written from scratch. Lineage only: it doesn't change how it plays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
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

/// Which model each author is (`authors.json` in the arena dir, e.g.
/// `{"agent:bunny-sol": "GPT"}`), so every bot's name can start with the model that wrote it.
pub fn author_models(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read(dir.join("authors.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// `GPT-Snowshoe v1` for `Snowshoe v1` by a GPT agent; unchanged if it already says so.
pub fn with_model(name: &str, model: Option<&str>) -> String {
    match model {
        Some(m) if !name.to_lowercase().starts_with(&m.to_lowercase()) => format!("{m}-{name}"),
        _ => name.to_string(),
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

/// Longest display name, before the model prefix (`GPT-`), so names fit in tables.
pub const MAX_NAME: usize = 18;

/// Ids are lowercase letters, digits and dashes, and can't shadow a roster CPU or the
/// ids league generations used in old games (`g3`, `champion`).
pub fn check_id(id: &str) -> Result<(), String> {
    let shape = (2..=40).contains(&id.len())
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-');
    if !shape {
        return Err(format!("bad id '{id}': use 2-40 lowercase letters, digits and dashes"));
    }
    let generation = id.strip_prefix('g').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if id == "champion" || generation || roster().iter().any(|s| s.id == id) {
        return Err(format!("'{id}' is reserved for a roster CPU or an old league generation"));
    }
    Ok(())
}

pub fn check_name(name: &str) -> Result<(), String> {
    let n = name.trim().chars().count();
    if !(2..=MAX_NAME).contains(&n) {
        return Err(format!(
            "name '{name}' is {n} characters; use 2-{MAX_NAME}, e.g. 'Muskox Quarrywall' (the id carries the version, and the model prefix is added for you)"
        ));
    }
    Ok(())
}

/// Every parent must name a published version or a roster CPU, and not the bot itself.
pub fn check_parents(dir: &Path, id: &str, parents: &[String]) -> Result<(), String> {
    let known: BTreeSet<String> = load_all(dir).into_iter().map(|e| e.id).chain(roster().into_iter().map(|s| s.id)).collect();
    for p in parents {
        if p == id {
            return Err(format!("'{id}' can't be its own parent"));
        }
        if !known.contains(p) {
            return Err(format!("unknown parent '{p}': name published versions (`hexarena arena list`) or roster CPUs"));
        }
    }
    Ok(())
}

/// Sets which bots `id` takes its policy from. Versions are immutable in play, but
/// lineage is metadata, so authors can fill it in after publishing.
pub fn set_parents(dir: &Path, id: &str, parents: Vec<String>) -> Result<Entrant, String> {
    check_parents(dir, id, &parents)?;
    let path = entrant_path(dir, id);
    let bytes = std::fs::read(&path).map_err(|_| format!("no entrant '{id}'"))?;
    let mut e: Entrant = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    e.parents = parents;
    std::fs::write(&path, serde_json::to_vec_pretty(&e).unwrap()).map_err(|e| e.to_string())?;
    Ok(e)
}

/// Publishes `entrant`. With `exe`, that program is copied into the arena so the version
/// stays frozen, and plays the seat through `exe bot`. The entrant must survive a short
/// trial game before it is listed.
pub fn publish(dir: &Path, mut entrant: Entrant, exe: Option<&Path>) -> Result<Entrant, String> {
    check_id(&entrant.id)?;
    check_name(&entrant.name)?;
    check_parents(dir, &entrant.id, &entrant.parents)?;
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

/// Why and when a roster CPU left the arena.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Retirement {
    pub reason: String,
    pub at: u64,
    /// The rules it was ranked under. A new rules version gives every roster CPU a fresh
    /// trial.
    #[serde(default)]
    pub rules: String,
}

fn all_retired_others(dir: &Path) -> BTreeMap<String, Retirement> {
    std::fs::read(dir.join("retired.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Roster CPUs retired under the current rules, from
/// `retired.json`.
pub fn retired_others(dir: &Path) -> BTreeMap<String, Retirement> {
    let rules = RulesConfig::default().version;
    all_retired_others(dir).into_iter().filter(|(_, r)| r.rules == rules).collect()
}

pub fn retire_other(dir: &Path, id: &str, reason: String, at: u64) -> Result<(), String> {
    let mut all = all_retired_others(dir);
    all.insert(id.to_string(), Retirement { reason, at, rules: RulesConfig::default().version });
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

    #[test]
    fn names_fit_in_tables() {
        assert!(check_name("Muskox Quarrywall").is_ok());
        assert!(check_name("Snowshoe v43 — Muskox Quarrywall").is_err());
        assert!(check_name("x").is_err());
    }
}
