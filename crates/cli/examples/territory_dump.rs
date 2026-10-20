//! Dumps every alive seat's territory at fixed ticks as JSON lines, for offline layout
//! optimisation (`scripts/layout/`).
//!
//! cargo run --release -p hexarena-cli --example territory_dump -- r1.replay.json [r2 ...] > territories.jsonl
//!
//! Each line: match id, tick, seat count, bot name, per-tick income, and owned hexes as
//! [q, r, height, building, border] where building is "" / "farm" / "village" / "quarry" /
//! "wall" and border is 1 when a neighbour is passable and not owned by the seat.

use hexarena_engine::State;
use hexarena_protocol::*;
use serde_json::json;

const SAMPLES: [u32; 3] = [3000, 6000, 9000];

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(replay) = serde_json::from_slice::<Replay>(&bytes) else { continue };
        if replay.rules.hash() != replay.rules_hash {
            continue;
        }
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let Ok(mut st) = State::new(replay.settings(), names.clone()) else { continue };
        let id = std::path::Path::new(&path)
            .file_name()
            .and_then(|f| f.to_str())
            .and_then(|f| f.split('.').next())
            .unwrap_or("")
            .to_string();
        let mut next = 0;
        for tick in &replay.commands {
            st.step(tick);
            if next < SAMPLES.len() && st.tick == SAMPLES[next] {
                dump(&st, &id, &names);
                next += 1;
            }
        }
    }
}

fn dump(st: &State, id: &str, names: &[String]) {
    let passable = |h: Hex| st.board.index(h).and_then(|i| st.terrain[i]).is_some_and(|t| t.passable());
    for (p, name) in names.iter().enumerate() {
        let p = p as PlayerId;
        if st.eliminated[p as usize] {
            continue;
        }
        let mut hexes = vec![];
        let mut income = 0;
        for h in st.board.hexes() {
            let Some(c) = st.cell(h) else { continue };
            if c.owner != Some(p) {
                continue;
            }
            income += st.income(h);
            let height = st.board.index(h).and_then(|i| st.terrain[i]).map_or(0, |t| t.height);
            let b = match c.building {
                Some(Building::Farm) => "farm",
                Some(Building::Village) => "village",
                Some(Building::Quarry) => "quarry",
                Some(Building::Wall) => "wall",
                None => "",
            };
            let border = h
                .neighbors()
                .iter()
                .any(|&n| passable(n) && st.cell(n).is_some_and(|c| c.owner != Some(p)));
            hexes.push(json!([h.q, h.r, height, b, border as u8]));
        }
        println!(
            "{}",
            json!({"match": id, "tick": st.tick, "seats": names.len(), "bot": name, "income": income, "hexes": hexes})
        );
    }
}
