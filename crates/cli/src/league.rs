//! `hexarena league <run|eval|submit|show>`.

use crate::get;
use hexarena_league::eval::{gate, Entrant, GateConfig};
use hexarena_league::{now_ms, roster_entrants, League, Submission};
use hexarena_sdk::BotParams;
use std::collections::HashMap;
use std::path::PathBuf;

const USAGE: &str = "usage: hexarena league <run|eval|submit|show> [--dir data/league]
  run    [--attempts N]                          keep evolving (forever by default)
  eval   --params cand.json [--against base.json] [--pairs N] [--seed S]
         gate a genome against the champion on fixed seeds without changing the league
  submit --params cand.json --origin agent:NAME [--note TEXT]
         queue a genome for the running league's next official gate
  show                                           champion and progression summary";

fn dir(f: &HashMap<String, String>) -> PathBuf {
    PathBuf::from(f.get("dir").cloned().unwrap_or_else(|| "data/league".into()))
}

/// Reads either bare `BotParams` or a `{ "params": ... }` wrapper.
pub(crate) fn read_params(path: &str) -> Result<BotParams, String> {
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| format!("{path}: {e}"))?)
            .map_err(|e| format!("{path}: {e}"))?;
    let inner = v.get("params").cloned().unwrap_or(v);
    serde_json::from_value(inner).map_err(|e| format!("{path}: {e}"))
}

pub fn main(rest: &[String]) -> Result<(), String> {
    let f = crate::flags(rest.get(1..).unwrap_or(&[]));
    match rest.first().map(String::as_str) {
        Some("run") => run(&f),
        Some("eval") => eval(&f),
        Some("submit") => submit(&f),
        Some("show") => show(&f),
        _ => Err(USAGE.into()),
    }
}

fn run(f: &HashMap<String, String>) -> Result<(), String> {
    let dir = dir(f);
    let mut league = League::open(&dir)?;
    let limit: u64 = get(f, "attempts", u64::MAX);
    println!(
        "league in {} — champion {} (benchmark AVP {:.2})",
        dir.display(),
        league.champion().id,
        league.champion().benchmark.avp
    );
    for _ in 0..limit {
        let a = league.attempt(&dir)?;
        let verdict = match a.became {
            Some(g) => format!("PROMOTED → g{g} (benchmark AVP {:.2})", league.champion().benchmark.avp),
            None => "rejected".into(),
        };
        println!(
            "#{} {} {:<10} gain {:+.2}±{:.2} over {} games in {:.1}s [{}] {}",
            a.n,
            a.id,
            a.origin,
            a.gate.gain,
            a.gate.gain_se,
            a.gate.games,
            a.gate.secs,
            a.changes.join(", "),
            verdict
        );
    }
    Ok(())
}

fn eval(f: &HashMap<String, String>) -> Result<(), String> {
    let path = f.get("params").ok_or(USAGE)?;
    let dir = dir(f);
    let league = League::load(&dir).ok();
    let base = match f.get("against") {
        Some(p) => Entrant { id: "base".into(), params: read_params(p)? },
        None => league
            .as_ref()
            .map(|l| l.champion_entrant())
            .unwrap_or(Entrant { id: "g0".into(), params: BotParams::default() }),
    };
    let candidate = Entrant { id: "candidate".into(), params: read_params(path)? };
    let pool = league.as_ref().map(|l| l.pool()).unwrap_or_else(roster_entrants);
    let pairs: usize = get(f, "pairs", 24);
    let cfg = GateConfig { min_pairs: pairs, max_pairs: pairs, batch_pairs: pairs.min(12), ..GateConfig::default() };
    let (r, _) = gate(&candidate, &base, &pool, get(f, "seed", 500_000), &cfg)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "against": base.id,
            "games": r.games,
            "candidate_avp": r.challenger_avp,
            "base_avp": r.champion_avp,
            "gain": r.gain,
            "gain_se": r.gain_se,
            "would_promote": r.promoted,
            "secs": r.secs,
        }))
        .unwrap()
    );
    Ok(())
}

fn submit(f: &HashMap<String, String>) -> Result<(), String> {
    let params = read_params(f.get("params").ok_or(USAGE)?)?;
    let origin = f.get("origin").cloned().unwrap_or_else(|| "agent:anonymous".into());
    let sub = Submission { origin: origin.clone(), note: f.get("note").cloned(), params };
    let inbox = dir(f).join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|e| e.to_string())?;
    let safe: String = origin.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let path = inbox.join(format!("{}-{safe}.json", now_ms()));
    std::fs::write(&path, serde_json::to_vec_pretty(&sub).unwrap()).map_err(|e| e.to_string())?;
    println!("queued {}", path.display());
    Ok(())
}

fn show(f: &HashMap<String, String>) -> Result<(), String> {
    let l = League::load(&dir(f))?;
    println!(
        "{} generations, {} attempts, {} games ({:.1} games/s)",
        l.generations.len(),
        l.attempts_total,
        l.games_total,
        l.games_total as f64 / l.play_secs.max(1e-9)
    );
    for g in &l.generations {
        println!(
            "{:>4} {:<10} elo {:>6.0}  benchmark AVP {:.2} win {:>3.0}% top-half {:>3.0}%  {}",
            g.id,
            g.origin,
            g.elo,
            g.benchmark.avp,
            g.benchmark.win_rate * 100.0,
            g.benchmark.top_half_rate * 100.0,
            g.changes.join(", ")
        );
    }
    println!("champion params:\n{}", serde_json::to_string_pretty(&l.champion().params).unwrap());
    Ok(())
}
