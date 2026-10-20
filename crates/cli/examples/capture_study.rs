//! Rival-capture economics per bot: troop cost per capture and what the captured hexes
//! looked like (defenders, height step, wall, how fresh), plus repelled attacks.
//!
//! cargo run --release -p hexarena-cli --example capture_study -- [--mid] r1.json [r2.json ...]
//! `--mid` keeps only ticks 3000-8999. Seats are keyed by the first word of their name.

use std::collections::BTreeMap;

use hexarena_engine::State;
use hexarena_protocol::*;

#[derive(Default)]
struct Agg {
    seats: u64,
    caps: u64,
    cost: u64,
    def: u64,
    left: u64,
    fresh: u64,
    wall: u64,
    weak: u64,
    by_dh: [(u64, u64); 3],
    repels: u64,
    repel_lost: u64,
    repel_def: u64,
}

fn main() {
    let mut mid = false;
    let mut paths = vec![];
    for a in std::env::args().skip(1) {
        if a == "--mid" {
            mid = true;
        } else {
            paths.push(a);
        }
    }
    let mut agg: BTreeMap<String, Agg> = BTreeMap::new();
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(replay) = serde_json::from_slice::<Replay>(&bytes) else { continue };
        let names: Vec<String> =
            replay.players.iter().map(|p| p.name.split(' ').next().unwrap_or("").to_string()).collect();
        if names.len() != 8 {
            continue;
        }
        for n in &names {
            agg.entry(n.clone()).or_default().seats += 1;
        }
        let Ok(mut st) = State::new(replay.settings(), names.clone()) else { continue };
        let slots = st.board.slots();
        let mut last_cap: Vec<u32> = vec![u32::MAX; slots];
        for cmds in &replay.commands {
            let pre = st.clone();
            let t = pre.tick;
            st.step(cmds);
            let keep = !mid || (3000..9000).contains(&t);
            let mut sent: BTreeMap<(PlayerId, Hex), (Hex, u32)> = BTreeMap::new();
            for e in &st.last_events {
                match e {
                    Event::Moved { player, from, to, troops } => {
                        let s = sent.entry((*player, *to)).or_insert((*from, 0));
                        s.1 += troops;
                    }
                    Event::Captured { player, at, previous_owner: Some(_), troops_left, .. } => {
                        let i = st.board.index(*at).unwrap();
                        let fresh = last_cap[i] != u32::MAX && t - last_cap[i] <= 30;
                        last_cap[i] = t;
                        let Some(&(from, troops)) = sent.get(&(*player, *at)) else { continue };
                        if !keep {
                            continue;
                        }
                        let a = agg.get_mut(&names[*player as usize]).unwrap();
                        let cost = troops.saturating_sub(*troops_left) as u64;
                        let def = pre.cells[i].troops as u64;
                        let dh = pre.height(from) as i32 - pre.height(*at) as i32;
                        a.caps += 1;
                        a.cost += cost;
                        a.def += def;
                        a.left += *troops_left as u64;
                        a.fresh += fresh as u64;
                        a.wall += (pre.cells[i].building == Some(Building::Wall)) as u64;
                        a.weak += (def <= 5) as u64;
                        let b = &mut a.by_dh[(dh.signum() + 1) as usize];
                        b.0 += 1;
                        b.1 += cost;
                    }
                    Event::Captured { at, .. } => {
                        last_cap[st.board.index(*at).unwrap()] = t;
                    }
                    Event::Repelled { attacker, at, attackers_lost } if keep => {
                        let i = st.board.index(*at).unwrap();
                        let a = agg.get_mut(&names[*attacker as usize]).unwrap();
                        a.repels += 1;
                        a.repel_lost += *attackers_lost as u64;
                        a.repel_def += pre.cells[i].troops as u64;
                    }
                    _ => {}
                }
            }
        }
    }
    println!(
        "{:16} {:>5} {:>7} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>17} {:>7} {:>7} {:>7}",
        "bot", "seats", "caps/g", "cost", "def", "left", "fresh%", "weak%", "wall%", "cost dn/flat/up", "rep/g", "replost", "repdef"
    );
    let mut rows: Vec<_> = agg.iter().filter(|(_, a)| a.seats >= 5 && a.caps > 0).collect();
    rows.sort_by_key(|(_, a)| std::cmp::Reverse(a.seats));
    for (b, a) in rows {
        let c = a.caps as f64;
        let per = |x: (u64, u64)| if x.0 == 0 { 0.0 } else { x.1 as f64 / x.0 as f64 };
        println!(
            "{:16} {:5} {:7.0} {:6.0} {:6.0} {:6.0} {:6.1} {:6.1} {:5.1} {:5.0}/{:5.0}/{:5.0} {:7.0} {:7.1} {:7.0}",
            b,
            a.seats,
            c / a.seats as f64,
            a.cost as f64 / c,
            a.def as f64 / c,
            a.left as f64 / c,
            100.0 * a.fresh as f64 / c,
            100.0 * a.weak as f64 / c,
            100.0 * a.wall as f64 / c,
            per(a.by_dh[2]),
            per(a.by_dh[1]),
            per(a.by_dh[0]),
            a.repels as f64 / a.seats as f64,
            a.repel_lost as f64 / a.repels.max(1) as f64,
            a.repel_def as f64 / a.repels.max(1) as f64,
        );
    }
}
