//! How good was each seat's start? Per seat: passable hexes within 4 and 6 steps of the
//! spawn (walking passable land), distance to the nearest rival spawn and to the board
//! edge, then its final place and elimination tick. One tab-separated row per seat.
//!
//! cargo run --release -p hexarena-cli --example start_study -- r1.json [r2.json ...]

use hexarena_engine::mapgen::passable;
use hexarena_engine::State;
use hexarena_protocol::*;
use std::collections::{HashMap, VecDeque};

fn reach(st: &State, from: &[Hex], steps: u32) -> usize {
    let mut dist: HashMap<Hex, u32> = from.iter().map(|&h| (h, 0)).collect();
    let mut q: VecDeque<Hex> = from.iter().copied().collect();
    while let Some(c) = q.pop_front() {
        let d = dist[&c];
        if d == steps {
            continue;
        }
        for n in c.neighbors() {
            if passable(&st.board, &st.terrain, n) && !dist.contains_key(&n) {
                dist.insert(n, d + 1);
                q.push_back(n);
            }
        }
    }
    dist.len()
}

fn main() {
    println!("file\tseat\tname\topen4\topen6\trival\tedge\tplace\tdead");
    for path in std::env::args().skip(1) {
        let replay: Replay = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let st = State::new(replay.settings(), names.clone()).unwrap();
        let mut spawn: Vec<Vec<Hex>> = vec![vec![]; names.len()];
        for (i, c) in st.cells.iter().enumerate() {
            if let Some(o) = c.owner {
                spawn[o as usize].push(st.board.hex_at(i));
            }
        }
        for (p, hs) in spawn.iter().enumerate() {
            let rival = spawn
                .iter()
                .enumerate()
                .filter(|(q, _)| *q != p)
                .flat_map(|(_, o)| o.iter().flat_map(|a| hs.iter().map(move |b| a.distance(*b))))
                .min()
                .unwrap_or(0);
            let edge = hs.iter().map(|h| st.board.radius - h.distance(Hex::new(0, 0))).min().unwrap_or(0);
            let pl = replay.result.placements.iter().find(|x| x.player as usize == p).unwrap();
            println!(
                "{path}\t{p}\t{}\t{}\t{}\t{rival}\t{edge}\t{}\t{}",
                names[p],
                reach(&st, hs, 4),
                reach(&st, hs, 6),
                pl.place_start,
                pl.eliminated_at.map_or(-1, |t| t as i64)
            );
        }
    }
}
