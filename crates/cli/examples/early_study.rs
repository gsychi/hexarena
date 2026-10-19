//! Opening of a finished game: land and troops of every seat every STEP ticks up to UNTIL,
//! plus the commands the seats whose name contains PREFIX sent in that window.
//!
//! cargo run --release -p hexarena-cli --example early_study -- replay.json PREFIX UNTIL [STEP]

use hexarena_engine::State;
use hexarena_protocol::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let replay: Replay = serde_json::from_slice(&std::fs::read(&args[0]).unwrap()).unwrap();
    let prefix = &args[1];
    let until: u32 = args[2].parse().unwrap();
    let step: u32 = args.get(3).map_or(25, |s| s.parse().unwrap());
    let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
    let n = names.len();
    let mine: Vec<usize> = (0..n).filter(|&i| names[i].contains(prefix.as_str())).collect();
    let mut st = State::new(replay.settings(), names.clone()).unwrap();
    for (i, nm) in names.iter().enumerate() {
        println!("p{i} {nm}");
    }
    for (t, tick) in replay.commands.iter().enumerate() {
        let t = t as u32;
        if t >= until {
            break;
        }
        for (p, c) in tick.iter() {
            if mine.contains(&(*p as usize)) && t % step < 3 {
                println!("  t{t} p{p} {:?}", c);
            }
        }
        st.step(tick);
        if t % step == 0 {
            let mut land = vec![0u32; n];
            let mut troops = vec![0u64; n];
            let mut builds = vec![0u32; n];
            for c in st.cells.iter() {
                if let Some(o) = c.owner {
                    land[o as usize] += 1;
                    troops[o as usize] += c.troops as u64;
                    builds[o as usize] += c.building.is_some() as u32;
                }
            }
            let row: Vec<String> = (0..n).map(|i| format!("{}/{}/{}", land[i], troops[i], builds[i])).collect();
            println!("t{t:<5} {}", row.join("  "));
        }
    }
}
