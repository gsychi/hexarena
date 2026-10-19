use hexarena_arena::{play, settings, viewer_json};
use hexarena_engine::stats::BuildingReport;
use hexarena_engine::{mapgen::generate, replay_state};
use hexarena_protocol::*;
use hexarena_sdk::{roster, style, Style};
use std::collections::HashMap;

mod league;

pub(crate) fn flags(args: &[String]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(k) = args[i].strip_prefix("--") {
            m.insert(k.to_string(), args.get(i + 1).cloned().unwrap_or_default());
            i += 1;
        }
        i += 1;
    }
    m
}

pub(crate) fn get<T: std::str::FromStr>(f: &HashMap<String, String>, k: &str, d: T) -> T {
    f.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// `--bots a,b,c` picks one CPU style per seat; otherwise `--players N` Expanders.
/// A seat: a roster id (`raider`), a league generation (`g3`, `champion`) from
/// `--league` (default `data/league`), or a genome file (`cand.json`).
fn seat_style(id: &str, f: &HashMap<String, String>) -> Result<Style, String> {
    if let Some(s) = style(id) {
        return Ok(s);
    }
    if id.ends_with(".json") {
        let name = std::path::Path::new(id)
            .file_stem()
            .map_or(id.to_string(), |s| s.to_string_lossy().to_string());
        return Ok(Style { id: name.clone(), name, blurb: String::new(), params: league::read_params(id)? });
    }
    let dir = f.get("league").cloned().unwrap_or_else(|| "data/league".into());
    let l = hexarena_league::League::load(std::path::Path::new(&dir))
        .map_err(|_| format!("unknown bot '{id}' (no league in {dir} to look it up)"))?;
    let g = if id == "champion" {
        l.champion()
    } else {
        l.generations.iter().find(|g| g.id == id).ok_or_else(|| format!("unknown bot '{id}'"))?
    };
    Ok(Style { id: g.id.clone(), name: format!("League {}", g.id), blurb: String::new(), params: g.params.clone() })
}

fn styles(f: &HashMap<String, String>) -> Result<Vec<Style>, String> {
    match f.get("bots") {
        Some(list) => list.split(',').map(|id| seat_style(id.trim(), f)).collect(),
        None => Ok(vec![roster().remove(0); get(f, "players", 4)]),
    }
}

fn describe(names: &[String], r: &MatchResult) -> String {
    r.placements
        .iter()
        .map(|p| {
            format!(
                "{}: place {}-{} ({:.1} pts, land {})",
                names[p.player as usize],
                p.place_start,
                p.place_end,
                p.points as f64 / 1000.0,
                p.land
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn simulate(f: HashMap<String, String>) -> Result<(), String> {
    let played = play(
        get(&f, "seed", 1),
        &styles(&f)?,
        get(&f, "max-ticks", u32::MAX),
    )?;
    let result = played.result();
    println!(
        "ticks: {}  winner: {:?}\n{}",
        result.final_tick,
        result.winner(),
        describe(&played.state.names, &result)
    );
    if let Some(path) = f.get("out") {
        std::fs::write(path, serde_json::to_vec(&played.replay()).unwrap())
            .map_err(|e| e.to_string())?;
        println!("replay written to {path}");
    }
    Ok(())
}

fn report(f: HashMap<String, String>) -> Result<(), String> {
    let n: u64 = get(&f, "matches", 10);
    let base: u64 = get(&f, "seed", 1);
    let seats = styles(&f)?;
    let mut total = BuildingReport::default();
    for i in 0..n {
        let played = play(base + i, &seats, get(&f, "max-ticks", u32::MAX))?;
        total.add_match(&played.state.records, &played.result());
    }
    total.finalize();
    println!("{}", serde_json::to_string_pretty(&total).unwrap());
    Ok(())
}

fn map(f: HashMap<String, String>) -> Result<(), String> {
    let m = generate(&settings(get(&f, "players", 4), get(&f, "seed", 1)))?;
    let open = m.terrain.iter().flatten().filter(|t| t.passable()).count();
    let total = m.terrain.iter().flatten().count();
    println!(
        "radius {} cells {} traversable {} ({}% impassable) spawns {:?}",
        m.radius,
        total,
        open,
        (total - open) * 100 / total,
        m.spawns
    );
    Ok(())
}

fn read_replay(path: &str) -> Result<Replay, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn verify(path: &str) -> Result<(), String> {
    let replay = read_replay(path)?;
    let st = replay_state(&replay)?;
    if format!("{:016x}", st.state_hash()) == replay.checksum {
        println!(
            "replay verified ({} ticks, rules {} {})",
            replay.commands.len(),
            replay.rules_version,
            replay.rules_hash
        );
        Ok(())
    } else {
        Err("replay checksum mismatch".into())
    }
}

fn export(path: &str, out: &str) -> Result<(), String> {
    let doc = viewer_json(&read_replay(path)?)?;
    std::fs::write(out, serde_json::to_vec(&doc).unwrap()).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    let res = match args.first().map(String::as_str) {
        Some("simulate") => simulate(flags(rest)),
        Some("report") => report(flags(rest)),
        Some("map") => map(flags(rest)),
        Some("league") => league::main(rest),
        Some("verify") => rest.first().map(|p| verify(p)).unwrap_or(Err("usage: hexarena verify <replay.json>".into())),
        Some("export") => match (rest.first(), rest.get(1)) {
            (Some(a), Some(b)) => export(a, b),
            _ => Err("usage: hexarena export <replay.json> <viewer.json>".into()),
        },
        _ => Err("usage: hexarena <simulate|report|map|verify|export|league> [--players N | --bots expander,raider,... --seed S --matches M --max-ticks T --out file]".into()),
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
