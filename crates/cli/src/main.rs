use hexarena_engine::{mapgen::generate, replay_state, State};
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

fn settings(f: &HashMap<String, String>) -> MatchSettings {
    let seed: u64 = get(f, "seed", 1);
    MatchSettings {
        players: get(f, "players", 4),
        map_seed: seed,
        spawn_seed: seed.wrapping_mul(31).wrapping_add(7),
        rules: RulesConfig::default(),
    }
}

fn simulate(f: HashMap<String, String>) -> Result<(), String> {
    let s = settings(&f);
    let mut st = State::new(s.clone(), vec![])?;
    let mut bots: Vec<Box<dyn Bot>> = (0..s.players)
        .map(|_| Box::new(Expander::default()) as Box<dyn Bot>)
        .collect();
    let mut log = vec![];
    let max_ticks: u32 = get(&f, "max-ticks", u32::MAX);
    while !st.is_over() && st.tick < max_ticks {
        let acts: Vec<(PlayerId, Action)> = (0..s.players)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| (p, bots[p as usize].act(&st.view(p))))
            .collect();
        st.step(&acts);
        log.push(acts);
    }
    let result = st.result.clone().unwrap_or_else(|| st.final_result());
    println!(
        "winner: {:?}  ticks: {}  hexes: {:?}\nplacement order: {:?}\npoints: {:?}",
        result.winner, result.ticks, result.final_hexes, result.placement_order, result.points
    );
    if let Some(path) = f.get("out") {
        let replay = Replay {
            protocol_version: PROTOCOL_VERSION,
            settings: s,
            names: st.names.clone(),
            actions: log,
            result,
            final_state_hash: st.state_hash(),
        };
        std::fs::write(path, serde_json::to_vec(&replay).unwrap()).map_err(|e| e.to_string())?;
        println!("replay written to {path}");
    }
    Ok(())
}

fn map(f: HashMap<String, String>) -> Result<(), String> {
    let s = settings(&f);
    let m = generate(&s)?;
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

fn verify(path: &str) -> Result<(), String> {
    let replay: Replay = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let st = replay_state(&replay)?;
    if st.state_hash() == replay.final_state_hash {
        println!("replay verified ({} ticks)", replay.actions.len());
        Ok(())
    } else {
        Err("replay hash mismatch".into())
    }
}

fn export(path: &str, out: &str) -> Result<(), String> {
    let replay: Replay = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut st = State::new(replay.settings.clone(), replay.names.clone())?;
    let code = |b: Option<Building>| b.map(|b| b as i32 + 1).unwrap_or(0);
    let frame = |st: &State| -> Vec<i32> {
        st.board
            .hexes()
            .iter()
            .filter_map(|&h| {
                let c = st.cell(h).unwrap();
                let i = st.board.index(h).unwrap() as i32;
                (c.owner.is_some() || c.building.is_some()).then(|| {
                    [
                        i,
                        c.owner.map(|o| o as i32).unwrap_or(-1),
                        c.troops as i32,
                        code(c.building),
                    ]
                })
            })
            .flatten()
            .collect()
    };
    let mut frames = vec![frame(&st)];
    let mut events = vec![serde_json::json!([])];
    for a in &replay.actions {
        st.step(a);
        frames.push(frame(&st));
        events.push(serde_json::to_value(&st.last_events).unwrap());
    }
    let doc = serde_json::json!({
        "radius": st.board.radius, "vision": st.rules().vision_radius, "names": st.names,
        "terrain": st.terrain, "neutral": st.board.hexes().iter().map(|&h| st.cell(h).unwrap().troops).collect::<Vec<_>>(),
        "frames": frames, "events": events,
        "result": replay.result, "rules_version": st.rules().rules_version,
    });
    std::fs::write(out, serde_json::to_vec(&doc).unwrap()).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    let res = match args.first().map(String::as_str) {
        Some("simulate") => simulate(flags(rest)),
        Some("map") => map(flags(rest)),
        Some("verify") => rest
            .first()
            .map(|p| verify(p))
            .unwrap_or(Err("usage: hexarena verify <replay.json>".into())),
        Some("export") => match (rest.first(), rest.get(1)) {
            (Some(a), Some(b)) => export(a, b),
            _ => Err("usage: hexarena export <replay.json> <viewer.json>".into()),
        },
        _ => Err(
            "usage: hexarena <simulate|map|verify|export> [--players N --seed S --out file]".into(),
        ),
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
