//! League generations as playable CPUs, and the league snapshot for the web page.

use hexarena_league::League;
use hexarena_sdk::{roster, style, Style};
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
        name: format!("League {}", g.id),
        blurb: format!(
            "Generation {} of the league, evolved from {}.",
            g.gen,
            g.parent.as_deref().unwrap_or("the hand-written Expander")
        ),
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
            "name": format!("League champion ({})", c.id),
            "blurb": format!("The strongest genome the league has evolved so far: generation {}.", c.gen),
        }));
    }
    out
}

pub fn snapshot(mut league: League) -> serde_json::Value {
    let keep = league.attempts.len().saturating_sub(SNAPSHOT_ATTEMPTS);
    league.attempts.drain(..keep);
    serde_json::to_value(league).unwrap()
}
