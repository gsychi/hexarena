//! `hexarena arena <publish|retire|list>`: put bot versions into the self-playing arena.

use crate::league::read_params;
use hexarena_arena::entrants::{load_all, publish, retire, Entrant};
use hexarena_league::now_ms;
use hexarena_protocol::RulesConfig;
use hexarena_sdk::BotParams;
use std::collections::HashMap;
use std::path::PathBuf;

const USAGE: &str = "usage: hexarena arena <publish|retire|list> [--dir data/arena]
  publish --id predator-v2 --name \"Predator v2\" --author agent:NAME
          (--params cand.json | --brain predator) [--note TEXT] [--builtin]
          freezes a copy of this hexarena binary, with the brains compiled into it, and
          lists the version in the arena; --builtin plays it with the server's own brains
  retire  --id predator-v2      stop scheduling a version (its results stay); the server
                                also retires versions outside the top 20 after 12 games
  list                          every published version
--dir defaults to $HEXARENA_ARENA or data/arena; the server reads the same directory.";

fn dir(f: &HashMap<String, String>) -> PathBuf {
    f.get("dir")
        .cloned()
        .or_else(|| std::env::var("HEXARENA_ARENA").ok())
        .unwrap_or_else(|| "data/arena".into())
        .into()
}

pub fn main(rest: &[String]) -> Result<(), String> {
    let f = crate::flags(rest.get(1..).unwrap_or(&[]));
    match rest.first().map(String::as_str) {
        Some("publish") => publish_cmd(&f),
        Some("retire") => {
            let e = retire(&dir(&f), f.get("id").ok_or(USAGE)?, Some("retired by hand".into()))?;
            println!("retired {} ({})", e.id, e.name);
            Ok(())
        }
        Some("list") => {
            for e in load_all(&dir(&f)) {
                let how = if e.program.is_some() { "own build" } else { "server brains" };
                let state = if e.retired { "retired" } else { "active" };
                println!("{:<24} {:<28} {:<20} {:<8} {:<13} {}", e.id, e.name, e.author, state, how, e.note.unwrap_or_default());
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn publish_cmd(f: &HashMap<String, String>) -> Result<(), String> {
    let id = f.get("id").ok_or(USAGE)?.clone();
    let params = match (f.get("params"), f.get("brain")) {
        (Some(p), _) => read_params(p)?,
        (None, Some(b)) => BotParams { brain: b.clone(), ..BotParams::default() },
        (None, None) => return Err(USAGE.into()),
    };
    let entrant = Entrant {
        name: f.get("name").cloned().unwrap_or_else(|| id.clone()),
        id,
        author: f.get("author").cloned().unwrap_or_else(|| "anonymous".into()),
        note: f.get("note").cloned(),
        created_at: now_ms(),
        retired: false,
        retired_reason: None,
        params,
        program: None,
    };
    let exe = if f.contains_key("builtin") { None } else { Some(std::env::current_exe().map_err(|e| e.to_string())?) };
    let e = publish(&dir(f), entrant, exe.as_deref())?;
    println!(
        "published {} ({}) by {}; the arena schedules it in its next games{}",
        e.id,
        e.name,
        e.author,
        e.program.map(|p| format!(", played by {}", p.exe)).unwrap_or_default()
    );
    print_briefing(&dir(f));
    Ok(())
}

/// Live rules and the newest lead notes, so an author publishing mid-run hears about
/// rule changes without having to go and read the lab notebook.
fn print_briefing(dir: &std::path::Path) {
    println!("live rules: {}", RulesConfig::default().version);
    let Ok(text) = std::fs::read_to_string(dir.join("observations.jsonl")) else { return };
    let lead: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|n| n["author"].as_str().is_some_and(|a| a == "agent:lead"))
        .collect();
    for n in lead.iter().rev().take(2).rev() {
        println!("lab notebook, agent:lead: {}", n["text"].as_str().unwrap_or_default());
    }
    println!("read every note with: curl -s http://127.0.0.1:47240/api/observations");
}
