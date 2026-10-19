//! Per-seat build-action counts from arena replay files.
//!
//! Usage:
//! cargo run --release -p hexarena-cli --example bunny_builds -- data/matches/*.replay.json

use hexarena_protocol::{Action, Building, Replay};
use std::collections::BTreeMap;

#[derive(Default)]
struct Counts {
    total: u32,
    farms: u32,
    villages: u32,
    quarries: u32,
    walls: u32,
}

fn main() {
    println!("replay,player,builds,farms,villages,quarries,walls,final_tick,place");
    let mut summary: BTreeMap<String, (u32, Counts)> = BTreeMap::new();
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read replay");
        let replay: Replay = serde_json::from_slice(&bytes).expect("parse replay");
        let mut counts: BTreeMap<u16, Counts> = BTreeMap::new();
        for commands in &replay.commands {
            for (player, command) in commands {
                let Action::Build { building, .. } = &command.action else {
                    continue;
                };
                let c = counts.entry(*player).or_default();
                c.total += 1;
                match building {
                    Building::Farm => c.farms += 1,
                    Building::Village => c.villages += 1,
                    Building::Quarry => c.quarries += 1,
                    Building::Wall => c.walls += 1,
                }
            }
        }
        for player in &replay.players {
            if !player.name.starts_with("Snowshoe") {
                continue;
            }
            let c = counts.entry(player.id).or_default();
            let place = replay.result.place_f64(player.id);
            let total = summary.entry(player.name.clone()).or_default();
            total.0 += 1;
            total.1.total += c.total;
            total.1.farms += c.farms;
            total.1.villages += c.villages;
            total.1.quarries += c.quarries;
            total.1.walls += c.walls;
            println!(
                "{},{},{},{},{},{},{},{},{place:.1}",
                path,
                player.name,
                c.total,
                c.farms,
                c.villages,
                c.quarries,
                c.walls,
                replay.result.final_tick,
            );
        }
    }
    println!("\nplayer,games,builds_per_game,farms_per_game,villages_per_game,quarries_per_game,walls_per_game");
    for (name, (games, c)) in summary {
        let n = games as f64;
        println!(
            "{name},{games},{:.3},{:.3},{:.3},{:.3},{:.3}",
            c.total as f64 / n,
            c.farms as f64 / n,
            c.villages as f64 / n,
            c.quarries as f64 / n,
            c.walls as f64 / n,
        );
    }
}
