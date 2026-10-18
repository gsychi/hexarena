use hexarena_engine::stats::BuildingReport;
use hexarena_engine::{build_replay, mapgen::generate, replay_state, State};
use hexarena_protocol::*;
use hexarena_sdk::{Bot, Expander};
use std::collections::HashMap;

fn flags(args: &[String]) -> HashMap<String, String> {
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

fn get<T: std::str::FromStr>(f: &HashMap<String, String>, k: &str, d: T) -> T {
    f.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn settings(f: &HashMap<String, String>, seed: u64) -> MatchSettings {
    MatchSettings {
        players: get(f, "players", 4),
        terrain_seed: seed,
        spawn_seed: seed.wrapping_mul(31).wrapping_add(7),
        match_seed: seed.wrapping_mul(13).wrapping_add(5),
        rules: RulesConfig::default(),
    }
}

struct Played {
    state: State,
    commands: Vec<TickCommands>,
    events: Vec<Vec<Event>>,
}

fn play(s: MatchSettings, max_ticks: u32) -> Result<Played, String> {
    let mut st = State::new(s.clone(), vec![])?;
    let mut bots: Vec<Box<dyn Bot>> = (0..s.players).map(|_| Box::new(Expander::default()) as Box<dyn Bot>).collect();
    let (mut commands, mut events) = (vec![], vec![]);
    while !st.is_over() && st.tick < max_ticks {
        let cmds: TickCommands = (0..s.players)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| (p, bots[p as usize].act(&st.view(p))))
            .collect();
        st.step(&cmds);
        commands.push(cmds);
        events.push(st.last_events.clone());
    }
    Ok(Played { state: st, commands, events })
}

fn describe(r: &MatchResult) -> String {
    r.placements
        .iter()
        .map(|p| format!("P{}: place {}-{} ({:.1} pts, land {})", p.player + 1, p.place_start, p.place_end, p.points as f64 / 1000.0, p.land))
        .collect::<Vec<_>>()
        .join("\n")
}

fn simulate(f: HashMap<String, String>) -> Result<(), String> {
    let s = settings(&f, get(&f, "seed", 1));
    let played = play(s, get(&f, "max-ticks", u32::MAX))?;
    let result = played.state.result.clone().unwrap_or_else(|| played.state.final_result());
    println!("ticks: {}  winner: {:?}\n{}", result.final_tick, result.winner(), describe(&result));
    if let Some(path) = f.get("out") {
        let replay = build_replay(&played.state, played.commands, played.events);
        std::fs::write(path, serde_json::to_vec(&replay).unwrap()).map_err(|e| e.to_string())?;
        println!("replay written to {path}");
    }
    Ok(())
}

fn report(f: HashMap<String, String>) -> Result<(), String> {
    let n: u64 = get(&f, "matches", 10);
    let base: u64 = get(&f, "seed", 1);
    let mut total = BuildingReport::default();
    for i in 0..n {
        let played = play(settings(&f, base + i), get(&f, "max-ticks", u32::MAX))?;
        let result = played.state.result.clone().unwrap_or_else(|| played.state.final_result());
        total.add_match(&played.state.records, &result);
    }
    total.finalize();
    println!("{}", serde_json::to_string_pretty(&total).unwrap());
    Ok(())
}

fn map(f: HashMap<String, String>) -> Result<(), String> {
    let s = settings(&f, get(&f, "seed", 1));
    let m = generate(&s)?;
    let open = m.terrain.iter().flatten().filter(|t| t.passable()).count();
    let total = m.terrain.iter().flatten().count();
    println!("radius {} cells {} traversable {} ({}% impassable) spawns {:?}", m.radius, total, open, (total - open) * 100 / total, m.spawns);
    Ok(())
}

fn read_replay(path: &str) -> Result<Replay, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn verify(path: &str) -> Result<(), String> {
    let replay = read_replay(path)?;
    let st = replay_state(&replay)?;
    if format!("{:016x}", st.state_hash()) == replay.checksum {
        println!("replay verified ({} ticks, rules {} {})", replay.commands.len(), replay.rules_version, replay.rules_hash);
        Ok(())
    } else {
        Err("replay checksum mismatch".into())
    }
}

/// Viewer frame stride: idx, owner, troops, building, production_remaining,
/// link a, link b, link c (hex indices or -1), link cooldown.
fn export(path: &str, out: &str) -> Result<(), String> {
    let replay = read_replay(path)?;
    let mut st = State::new(replay.settings(), replay.players.iter().map(|p| p.name.clone()).collect())?;
    let radius = st.board.radius;
    let frame = |st: &State| -> Vec<i32> {
        let mut v = vec![];
        for h in st.board.hexes() {
            let c = st.cell(h).unwrap();
            if c.owner.is_none() && c.building.is_none() {
                continue;
            }
            let cell = st_visible(st, h);
            v.extend([
                st.board.index(h).unwrap() as i32,
                c.owner.map(|o| o as i32).unwrap_or(-1),
                c.troops as i32,
                c.building.map(|b| b as i32 + 1).unwrap_or(0),
                cell.0 as i32,
                cell.1[0].map(|l| st.board.index(l).unwrap() as i32).unwrap_or(-1),
                cell.1[1].map(|l| st.board.index(l).unwrap() as i32).unwrap_or(-1),
                cell.1[2].map(|l| st.board.index(l).unwrap() as i32).unwrap_or(-1),
                cell.2 as i32,
            ]);
        }
        v
    };
    let mut frames = vec![frame(&st)];
    let mut events = vec![serde_json::json!([])];
    for cmds in &replay.commands {
        st.step(cmds);
        frames.push(frame(&st));
        events.push(serde_json::to_value(&st.last_events).unwrap());
    }
    let doc = serde_json::json!({
        "radius": radius, "fog": st.rules().fog_radius, "names": st.names, "rules": st.rules(),
        "terrain": st.terrain,
        "neutral": st.board.hexes().iter().map(|&h| st.cell(h).unwrap().troops).collect::<Vec<_>>(),
        "frames": frames, "events": events,
        "result": replay.result, "building_report": st.building_report(),
    });
    std::fs::write(out, serde_json::to_vec(&doc).unwrap()).map_err(|e| e.to_string())
}

/// (production remaining, links, cooldown) for a hex, using the same logic the View exposes.
fn st_visible(st: &State, h: Hex) -> (u32, [Option<Hex>; 3], u32) {
    let c = st.cell(h).unwrap();
    let remaining = match c.building {
        Some(Building::Farm | Building::Quarry) => st.rules().extractor_period.saturating_sub(c.progress),
        _ => 0,
    };
    let links = match c.building {
        Some(Building::Village) => [st.supporting_farm(h), None, None],
        _ => c.links,
    };
    (remaining, links, c.link_ready_tick.saturating_sub(st.tick))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    let res = match args.first().map(String::as_str) {
        Some("simulate") => simulate(flags(rest)),
        Some("report") => report(flags(rest)),
        Some("map") => map(flags(rest)),
        Some("verify") => rest.first().map(|p| verify(p)).unwrap_or(Err("usage: hexarena verify <replay.json>".into())),
        Some("export") => match (rest.first(), rest.get(1)) {
            (Some(a), Some(b)) => export(a, b),
            _ => Err("usage: hexarena export <replay.json> <viewer.json>".into()),
        },
        _ => Err("usage: hexarena <simulate|report|map|verify|export> [--players N --seed S --matches M --max-ticks T --out file]".into()),
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
