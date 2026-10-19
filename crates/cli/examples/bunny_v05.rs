//! Replay study for Bunny Lab's v0.5 batches.
//!
//! Reads `*.viewer.json` files and their neighboring `*.summary.json` sidecars, then
//! reports build portfolios, elimination/killer evidence, and land/troop-production
//! curves. Production is reconstructed from decoded frames under the live v0.5 rule:
//! one per owned hex, one extra on a Village, and one per adjacent friendly Farm.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

const FIELDS: usize = 8;
const PREDICTED: [usize; 3] = [1, 3, 7];
const REMOVED: i32 = 1 << 8;
const CHECKPOINTS: [usize; 5] = [1000, 3000, 5000, 7000, 9000];

#[derive(Clone, Copy, Default)]
struct Curve {
    land: usize,
    troops: i64,
    production: usize,
}

struct Frames {
    state: Vec<[i32; FIELDS]>,
    present: Vec<bool>,
    velocity: Vec<[i32; 3]>,
}

impl Frames {
    fn new(size: usize, first: &[Value]) -> Self {
        let mut this = Self {
            state: vec![[0; FIELDS]; size],
            present: vec![false; size],
            velocity: vec![[0; 3]; size],
        };
        for record in first.chunks(9) {
            let idx = number(&record[0]) as usize;
            this.present[idx] = true;
            for field in 0..FIELDS {
                this.state[idx][field] = number(&record[field + 1]);
            }
        }
        this
    }

    fn step(&mut self, delta: &[Value]) {
        let was = self.present.clone();
        let old: Vec<[i32; 3]> = self
            .state
            .iter()
            .map(|record| PREDICTED.map(|field| record[field]))
            .collect();
        for idx in 0..self.state.len() {
            if self.present[idx] {
                for (slot, field) in PREDICTED.into_iter().enumerate() {
                    self.state[idx][field] += self.velocity[idx][slot];
                }
            }
        }
        let mut at = 0;
        while at < delta.len() {
            let idx = number(&delta[at]) as usize;
            let mask = number(&delta[at + 1]);
            at += 2;
            if mask & REMOVED != 0 {
                self.present[idx] = false;
                continue;
            }
            if !self.present[idx] {
                self.state[idx] = [0; FIELDS];
                self.present[idx] = true;
            }
            for field in 0..FIELDS {
                if mask & (1 << field) != 0 {
                    self.state[idx][field] = number(&delta[at]);
                    at += 1;
                }
            }
        }
        for idx in 0..self.state.len() {
            self.velocity[idx] = if self.present[idx] && was[idx] {
                std::array::from_fn(|slot| self.state[idx][PREDICTED[slot]] - old[idx][slot])
            } else {
                [0; 3]
            };
        }
    }

    fn curve(&self, player: i32, radius: i32) -> Curve {
        let width = 2 * radius + 1;
        let mut curve = Curve::default();
        for idx in 0..self.state.len() {
            if !self.present[idx] || self.state[idx][0] != player {
                continue;
            }
            curve.land += 1;
            curve.troops += self.state[idx][1] as i64;
            curve.production += 1;
            if self.state[idx][2] == 2 {
                curve.production += 1;
                let q = idx as i32 % width - radius;
                let r = idx as i32 / width - radius;
                for (dq, dr) in [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)] {
                    let (nq, nr) = (q + dq, r + dr);
                    if nq.abs() > radius || nr.abs() > radius || (nq + nr).abs() > radius {
                        continue;
                    }
                    let neighbor = ((nr + radius) * width + nq + radius) as usize;
                    if self.present[neighbor]
                        && self.state[neighbor][0] == player
                        && self.state[neighbor][2] == 1
                    {
                        curve.production += 1;
                    }
                }
            }
        }
        curve
    }
}

fn number(value: &Value) -> i32 {
    value.as_i64().expect("integer") as i32
}

fn main() {
    println!(
        "match,bot,place,death,killer,builds,farms,villages,quarries,walls,\
         land1000,prod1000,troops1000,land3000,prod3000,troops3000,\
         land5000,prod5000,troops5000,land7000,prod7000,troops7000,\
         land9000,prod9000,troops9000"
    );
    for path in std::env::args().skip(1) {
        study(Path::new(&path));
    }
}

fn study(path: &Path) {
    let viewer: Value =
        serde_json::from_slice(&std::fs::read(path).expect("read viewer")).expect("viewer JSON");
    let summary_path = path
        .to_string_lossy()
        .replace(".viewer.json", ".summary.json");
    let summary: Value =
        serde_json::from_slice(&std::fs::read(&summary_path).expect("read neighboring summary"))
            .expect("summary JSON");
    let id = summary["id"].as_str().unwrap();
    let radius = number(&viewer["radius"]);
    let frames = viewer["frames"].as_array().unwrap();
    let mut decoded = Frames::new(
        ((2 * radius + 1) * (2 * radius + 1)) as usize,
        frames[0].as_array().unwrap(),
    );
    let mut curves: BTreeMap<(usize, usize), Curve> = BTreeMap::new();
    for (tick, delta) in frames.iter().enumerate().skip(1) {
        decoded.step(delta.as_array().unwrap());
        if CHECKPOINTS.contains(&tick) {
            for player in 0..viewer["names"].as_array().unwrap().len() {
                curves.insert((player, tick), decoded.curve(player as i32, radius));
            }
        }
    }

    for seat in summary["seats"].as_array().unwrap() {
        let bot = seat["bot"].as_str().unwrap();
        if !bot.starts_with("snowshoe-v") {
            continue;
        }
        let player = summary["seats"]
            .as_array()
            .unwrap()
            .iter()
            .position(|candidate| candidate["bot"] == seat["bot"])
            .unwrap();
        let placement = viewer["result"]["placements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| number(&p["player"]) as usize == player)
            .unwrap();
        let death = placement["eliminated_at"]
            .as_u64()
            .map(|tick| tick as usize);
        let killer = death
            .and_then(|tick| killer_at(&viewer["events"][tick], player))
            .and_then(|killer| summary["seats"][killer]["bot"].as_str())
            .unwrap_or("-");
        let mut builds: BTreeMap<&str, usize> = BTreeMap::new();
        for tick in viewer["events"].as_array().unwrap() {
            for event in tick.as_array().unwrap() {
                if event["type"] == "built" && number(&event["player"]) as usize == player {
                    *builds
                        .entry(event["building"].as_str().unwrap())
                        .or_default() += 1;
                }
            }
        }
        let total: usize = builds.values().sum();
        let place =
            (number(&placement["place_start"]) + number(&placement["place_end"])) as f64 / 2.0;
        print!(
            "{id},{bot},{place:.1},{},{killer},{total},{},{},{},{}",
            death.map_or("-".into(), |tick| tick.to_string()),
            builds.get("farm").copied().unwrap_or(0),
            builds.get("village").copied().unwrap_or(0),
            builds.get("quarry").copied().unwrap_or(0),
            builds.get("wall").copied().unwrap_or(0),
        );
        for tick in CHECKPOINTS {
            let curve = curves.get(&(player, tick)).copied().unwrap_or_default();
            print!(",{},{},{}", curve.land, curve.production, curve.troops);
        }
        println!();
    }
}

fn killer_at(events: &Value, victim: usize) -> Option<usize> {
    events.as_array().unwrap().iter().find_map(|event| {
        if event["type"] == "surrendered" && number(&event["from"]) as usize == victim {
            event["capturer"].as_u64().map(|player| player as usize)
        } else if event["type"] == "captured"
            && event["previous_owner"].as_u64() == Some(victim as u64)
        {
            event["player"].as_u64().map(|player| player as usize)
        } else {
            None
        }
    })
}
