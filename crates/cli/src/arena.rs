//! `hexarena arena <publish|parents|retire|list>` and `hexarena eval`: put bot versions
//! into the self-playing arena, and compare two bots locally first.

use crate::{get, seat_style};
use hexarena_arena::entrants::{load_all, publish, retire, set_parents, Entrant, MAX_NAME};
use hexarena_arena::eval::compare;
use hexarena_protocol::RulesConfig;
use hexarena_sdk::{roster, BotParams};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "usage: hexarena arena <publish|parents|retire|list> [--dir data/arena]
  publish --id snowshoe-v44 --name \"Muskox Quarrywall\" --author agent:NAME
          --parents snowshoe-v43,raider (or --parents none for a design from scratch)
          (--params cand.json | --brain predator) [--note TEXT] [--builtin]
          freezes a copy of this hexarena binary, with the brains compiled into it, and
          lists the version in the arena; --builtin plays it with the server's own brains.
          Names are 2-18 characters (the id carries the version, the model prefix is
          added for you). Parents are the bots it takes its policy from, most first.
  parents --id snowshoe-v43 --parents snowshoe-v40,raider   fill in lineage afterwards
  retire  --id snowshoe-v44     stop scheduling a version (its results stay); the server
                                also retires versions outside the top 20 after 50 games
  list                          every published version
--dir defaults to $HEXARENA_ARENA or data/arena; the server reads the same directory.";

const EVAL_USAGE: &str = "usage: hexarena eval --params cand.json [--against ID|base.json] [--pairs N] [--seed S] [--author agent:NAME]
  plays the candidate against a base (an arena version, roster CPU or genome file; the
  roster's first CPU by default) in mirrored pairs of 8-player lobbies: 3 seats each plus
  2 roster CPUs, then the same map with the two swapped. Reports the gain in 8-player
  places (positive is better; ending the game early earns up to 1.5 places). With an
  author (--author or HEXARENA_AUTHOR) the home page shows the test and its result.";

fn dir(f: &HashMap<String, String>) -> PathBuf {
    f.get("dir")
        .cloned()
        .or_else(|| std::env::var("HEXARENA_ARENA").ok())
        .unwrap_or_else(|| "data/arena".into())
        .into()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Reads either bare `BotParams` or a `{ "params": ... }` wrapper.
pub(crate) fn read_params(path: &str) -> Result<BotParams, String> {
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)
            .map_err(|e| format!("{path}: {e}"))?;
    let inner = v.get("params").cloned().unwrap_or(v);
    serde_json::from_value(inner).map_err(|e| format!("{path}: {e}"))
}

/// `a,b` → `[a, b]`; `none` → `[]`.
fn parent_list(raw: &str) -> Vec<String> {
    raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "none").collect()
}

pub fn main(rest: &[String]) -> Result<(), String> {
    let f = crate::flags(rest.get(1..).unwrap_or(&[]));
    match rest.first().map(String::as_str) {
        Some("publish") => publish_cmd(&f),
        Some("parents") => {
            let id = f.get("id").ok_or(USAGE)?;
            let e = set_parents(&dir(&f), id, parent_list(f.get("parents").ok_or(USAGE)?))?;
            println!("{} ({}) takes its policy from: {}", e.id, e.name, if e.parents.is_empty() { "nobody (from scratch)".into() } else { e.parents.join(", ") });
            Ok(())
        }
        Some("retire") => {
            let e = retire(&dir(&f), f.get("id").ok_or(USAGE)?, Some("retired by hand".into()))?;
            println!("retired {} ({})", e.id, e.name);
            Ok(())
        }
        Some("list") => {
            for e in load_all(&dir(&f)) {
                let how = if e.program.is_some() { "own build" } else { "server brains" };
                let state = if e.retired { "retired" } else { "active" };
                let parents = if e.parents.is_empty() { "-".into() } else { e.parents.join(",") };
                println!("{:<24} {:<20} {:<20} {:<8} {:<13} parents {:<28} {}", e.id, e.name, e.author, state, how, parents, e.note.unwrap_or_default());
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn publish_cmd(f: &HashMap<String, String>) -> Result<(), String> {
    let id = f.get("id").ok_or(USAGE)?.clone();
    let name = f.get("name").ok_or_else(|| format!("--name is required (2-{MAX_NAME} characters)\n{USAGE}"))?.trim().to_string();
    let parents = f.get("parents").ok_or_else(|| {
        format!("--parents is required: the bots this one takes its policy from, e.g. --parents snowshoe-v43,raider, or --parents none for a design from scratch\n{USAGE}")
    })?;
    let params = match (f.get("params"), f.get("brain")) {
        (Some(p), _) => read_params(p)?,
        (None, Some(b)) => BotParams { brain: b.clone(), ..BotParams::default() },
        (None, None) => return Err(USAGE.into()),
    };
    let entrant = Entrant {
        name,
        id,
        author: f.get("author").cloned().unwrap_or_else(|| "anonymous".into()),
        note: f.get("note").cloned(),
        created_at: now_ms(),
        retired: false,
        retired_reason: None,
        parents: parent_list(parents),
        params,
        program: None,
    };
    let exe = if f.contains_key("builtin") { None } else { Some(std::env::current_exe().map_err(|e| e.to_string())?) };
    let e = publish(&dir(f), entrant, exe.as_deref())?;
    println!(
        "published {} ({}) by {}, child of {}; the arena schedules it in its next games{}",
        e.id,
        e.name,
        e.author,
        if e.parents.is_empty() { "nobody (from scratch)".into() } else { e.parents.join(" × ") },
        e.program.map(|p| format!(", played by {}", p.exe)).unwrap_or_default()
    );
    print_briefing(&dir(f), &e.author);
    Ok(())
}

/// Live rules and the newest lead notes, so an author publishing mid-run hears about
/// rule changes without having to go and read the lab notebook.
fn print_briefing(dir: &std::path::Path, author: &str) {
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
    let thread: Vec<serde_json::Value> = std::fs::read_to_string(dir.join("messages.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["agent"] == author)
        .collect();
    let unanswered: Vec<&serde_json::Value> = thread.iter().rev().take_while(|m| m["from"] != author).collect();
    for m in unanswered.iter().rev() {
        println!("message for you from the arena: {}", m["text"].as_str().unwrap_or_default());
    }
    println!(
        "tell people what you are working on (shown on the home page): curl -s -X POST -H 'content-type: application/json' http://127.0.0.1:47240/api/agents/{author}/status -d '{{\"text\":\"...\"}}'"
    );
    if !unanswered.is_empty() {
        println!(
            "reply with: curl -s -X POST -H 'content-type: application/json' http://127.0.0.1:47240/api/agents/{author}/messages -d '{{\"from\":\"{author}\",\"text\":\"...\"}}'"
        );
    }
}

/// Tells the arena server what an agent's tools are doing, so the home page shows it live.
/// Best effort: a server that is down or slow never holds up the eval.
fn report_work(author: &str, text: &str) {
    use std::io::Write;
    let addr = std::env::var("HEXARENA_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:47240".into());
    let Ok(sock) = addr.parse::<std::net::SocketAddr>() else { return };
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(2)) else { return };
    let _ = s.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    let body = serde_json::json!({ "text": text }).to_string();
    let path = format!("/api/agents/{author}/work");
    let _ = write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // The server drops a request whose client hangs up before it answers.
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let _ = std::io::Read::read(&mut s, &mut [0u8; 512]);
}

pub fn eval(f: &HashMap<String, String>) -> Result<(), String> {
    let candidate = seat_style(f.get("params").ok_or(EVAL_USAGE)?, f)?;
    let base = match f.get("against") {
        Some(id) => seat_style(id, f)?,
        None => roster().remove(0),
    };
    let pairs: usize = get(f, "pairs", 24);
    let author = f.get("author").cloned().or_else(|| std::env::var("HEXARENA_AUTHOR").ok());
    let label = |s: &hexarena_sdk::Style| {
        let p = std::path::Path::new(&s.id);
        if p.extension().is_some() { p.file_stem().map_or(s.id.clone(), |x| x.to_string_lossy().into_owned()) } else { s.id.clone() }
    };
    let what = format!("{} against {}", label(&candidate), label(&base));
    if let Some(a) = &author {
        report_work(a, &format!("Testing {what} locally ({pairs} mirrored pairs of 8-player games)"));
    }
    let (r, _) = compare(&candidate, &base, &roster(), get(f, "seed", 500_000), pairs)?;
    if let Some(a) = &author {
        report_work(
            a,
            &format!(
                "Tested {what}: {:+.2} ± {} places over {} games (closes {} vs {})",
                r.gain,
                if r.gain_se.is_finite() { format!("{:.2}", r.gain_se) } else { "n/a".into() },
                r.games,
                r.candidate_closes,
                r.base_closes
            ),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "against": base.id,
            "games": r.games,
            "candidate_score": r.candidate_score,
            "base_score": r.base_score,
            "gain": r.gain,
            "gain_se": r.gain_se,
            "candidate_closes": r.candidate_closes,
            "base_closes": r.base_closes,
            "secs": r.secs,
        }))
        .unwrap()
    );
    Ok(())
}
