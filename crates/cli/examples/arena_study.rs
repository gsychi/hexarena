//! Per-seat study of finished games: placement, when the seat was eliminated and by whom,
//! what it built and when, and how many of its buildings changed hands.
//!
//! cargo run --release -p hexarena-cli --example arena_study -- PREFIX r1.json [r2.json ...]
//! Seats whose name contains PREFIX are reported.

use hexarena_engine::State;
use hexarena_protocol::*;
use std::collections::BTreeMap;

fn main() {
    let mut args = std::env::args().skip(1);
    let prefix = args.next().unwrap();
    for path in args {
        let replay: Replay = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let n = names.len();
        let st = State::new(replay.settings(), names.clone()).unwrap();
        let mut owner: BTreeMap<Hex, PlayerId> = BTreeMap::new();
        for (i, c) in st.cells.iter().enumerate() {
            if let Some(o) = c.owner {
                owner.insert(st.board.hex_at(i), o);
            }
        }
        let mut land = vec![0i64; n];
        for o in owner.values() {
            land[*o as usize] += 1;
        }
        let mut peak = vec![(0i64, 0u32); n];
        let mut at_tick: Vec<BTreeMap<u32, i64>> = vec![BTreeMap::new(); n];
        let mut dead: Vec<Option<u32>> = vec![None; n];
        let mut built: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); n];
        let mut first_build = vec![None; n];
        let mut built_by_3000 = vec![0u32; n];
        let mut buildings_lost = vec![0u32; n];
        let mut buildings_taken = vec![0u32; n];
        let mut lost_to: Vec<BTreeMap<PlayerId, u32>> = vec![BTreeMap::new(); n];
        let mut late_lost_to: Vec<Vec<(u32, PlayerId)>> = vec![vec![]; n];
        let mut rejected = vec![0u32; n];
        for (t, evs) in replay.events.iter().enumerate() {
            let t = t as u32;
            for e in evs {
                match e {
                    Event::Captured { player, at, previous_owner, building, .. } => {
                        owner.insert(*at, *player);
                        land[*player as usize] += 1;
                        if let Some(p) = previous_owner {
                            land[*p as usize] -= 1;
                            *lost_to[*p as usize].entry(*player).or_default() += 1;
                            late_lost_to[*p as usize].push((t, *player));
                            if building.is_some() {
                                buildings_lost[*p as usize] += 1;
                                buildings_taken[*player as usize] += 1;
                            }
                        }
                    }
                    Event::Surrendered { capturer, from, hexes } => {
                        for h in hexes {
                            owner.insert(*h, *capturer);
                        }
                        land[*capturer as usize] += hexes.len() as i64;
                        land[*from as usize] -= hexes.len() as i64;
                        *lost_to[*from as usize].entry(*capturer).or_default() += hexes.len() as u32;
                        late_lost_to[*from as usize].extend(hexes.iter().map(|_| (t, *capturer)));
                    }
                    Event::Built { player, building, .. } => {
                        *built[*player as usize].entry(format!("{building:?}")).or_default() += 1;
                        first_build[*player as usize].get_or_insert(t);
                        if t < 3000 {
                            built_by_3000[*player as usize] += 1;
                        }
                    }
                    Event::Eliminated { player } => dead[*player as usize] = Some(t),
                    Event::Rejected { player, .. } => rejected[*player as usize] += 1,
                    _ => {}
                }
            }
            for p in 0..n {
                if land[p] > peak[p].0 {
                    peak[p] = (land[p], t);
                }
                if [1000, 3000, 5000, 7000].contains(&t) {
                    at_tick[p].insert(t, land[p]);
                }
            }
        }
        let res = &replay.result;
        let alive = (0..n).filter(|&p| dead[p].is_none()).count();
        println!("== {path}  final tick {}  alive at end {alive}", res.final_tick);
        for p in 0..n {
            if !names[p].contains(&prefix) {
                continue;
            }
            let pl = res.placement(p as PlayerId);
            let killer = dead[p].map(|d| {
                let mut c: BTreeMap<PlayerId, u32> = BTreeMap::new();
                for (t, k) in &late_lost_to[p] {
                    if t + 300 >= d {
                        *c.entry(*k).or_default() += 1;
                    }
                }
                c.into_iter().max_by_key(|x| x.1).map(|(k, _)| names[k as usize].clone()).unwrap_or_default()
            });
            let top_lost: Vec<String> = {
                let mut v: Vec<_> = lost_to[p].iter().collect();
                v.sort_by_key(|x| std::cmp::Reverse(*x.1));
                v.iter().take(3).map(|(k, c)| format!("{}:{}", short(&names[**k as usize]), c)).collect()
            };
            println!(
                "  {:<28} place {} land {} peak {}@{} land@1k/3k/5k/7k {:?} dead {:?} killer {:?}",
                short(&names[p]),
                pl.place_start,
                land[p],
                peak[p].0,
                peak[p].1,
                at_tick[p].values().collect::<Vec<_>>(),
                dead[p],
                killer.map(|k| short(&k)),
            );
            println!(
                "      builds {:?} first@{:?} before3k {} lost {} taken {} rejected {} lost_to {:?}",
                built[p], first_build[p], built_by_3000[p], buildings_lost[p], buildings_taken[p], rejected[p], top_lost
            );
        }
    }
}

fn short(s: &str) -> String {
    s.split(" —").next().unwrap_or(s).to_string()
}
