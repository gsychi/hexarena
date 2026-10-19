//! Churn analysis: how quickly captured hexes change hands again, what garrison the lost
//! hexes had, and how many hexes a seat loses right after its own Gather.
//!
//! cargo run --release -p hexarena-cli --example churn_stats -- PREFIX r1.json [r2.json ...]
//! Only seats whose name starts with PREFIX are aggregated.

use hexarena_engine::State;
use hexarena_protocol::*;

const BINS: usize = 12;

fn main() {
    let mut args = std::env::args().skip(1);
    let prefix = args.next().unwrap();
    let paths: Vec<String> = args.collect();
    let mut seats = 0u64;
    let mut cap_neutral = [0u64; BINS];
    let mut cap_enemy = [0u64; BINS];
    let mut lost = [0u64; BINS];
    let mut gathers = [0u64; BINS];
    let mut repelled = [0u64; BINS];
    let mut internal = [0u64; BINS];
    let mut noops = [0u64; BINS];
    let (mut lost_fresh, mut lost_total) = (0u64, 0u64);
    let mut lost_garrison_hist = [0u64; 8];
    let (mut recaps, mut caps_enemy_total) = (0u64, 0u64);
    let (mut loss_after_gather, mut gather_windows) = (0u64, 0u64);
    let (mut loss_base, mut base_ticks) = (0u64, 0u64);
    let mut lost_by_attacker_dh = std::collections::BTreeMap::<i8, u64>::new();
    let mut lost_def_height = std::collections::BTreeMap::<i8, u64>::new();
    let mut own_height = std::collections::BTreeMap::<i8, u64>::new();
    let mut src_left_frontier = (0u64, 0u64);
    let mut lost_to = std::collections::BTreeMap::<String, u64>::new();
    let mut caps_from = std::collections::BTreeMap::<String, u64>::new();
    for path in &paths {
        let replay: Replay = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let mine: Vec<bool> = names.iter().map(|n| n.starts_with(&prefix)).collect();
        seats += mine.iter().filter(|m| **m).count() as u64;
        let mut st = State::new(replay.settings(), names.clone()).unwrap();
        let slots = st.board.slots();
        // tick a hex was last captured, by whom
        let mut last_cap: Vec<Option<(u32, PlayerId)>> = vec![None; slots];
        let mut last_lost_by: Vec<Vec<Option<u32>>> = vec![vec![None; slots]; names.len()];
        let mut last_gather: Vec<Option<u32>> = vec![None; names.len()];
        for cmds in &replay.commands {
            let pre = st.clone();
            let t = pre.tick;
            let bin = (t as usize * BINS / 3000).min(BINS - 1);
            for (p, cmd) in cmds {
                if !mine[*p as usize] {
                    continue;
                }
                match &cmd.action {
                    Action::Move { to, .. } if pre.cell(*to).is_some_and(|c| c.owner == Some(*p)) => internal[bin] += 1,
                    Action::NoOp => noops[bin] += 1,
                    _ => {}
                }
                if let Action::Gather { .. } = cmd.action {
                    gathers[bin] += 1;
                    last_gather[*p as usize] = Some(t);
                }
            }
            st.step(cmds);
            let mut lost_this_tick = vec![0u64; names.len()];
            let mut last_move: std::collections::BTreeMap<PlayerId, (Hex, u32)> = Default::default();
            for e in &st.last_events {
                match e {
                    Event::Moved { player, from, troops, .. } => {
                        last_move.insert(*player, (*from, *troops));
                    }
                    Event::Captured { player, at, previous_owner, .. } => {
                        let i = st.board.index(*at).unwrap();
                        let (from, sent) = last_move[player];
                        if mine[*player as usize] {
                            if previous_owner.is_some() {
                                cap_enemy[bin] += 1;
                                caps_enemy_total += 1;
                                if last_lost_by[*player as usize][i].is_some_and(|lt| t - lt <= 30) {
                                    recaps += 1;
                                }
                            } else {
                                cap_neutral[bin] += 1;
                            }
                            let src_left = pre.cell(from).unwrap().troops - sent;
                            let still_front = from.neighbors().iter().any(|n| {
                                st.board.index(*n).is_some_and(|j| {
                                    st.terrain[j].is_some_and(|t| t.passable())
                                        && st.cells[j].owner != Some(*player)
                                })
                            });
                            if still_front {
                                src_left_frontier.0 += 1;
                                src_left_frontier.1 += src_left as u64;
                            }
                        }
                        if let Some(o) = previous_owner {
                            if mine[*player as usize] {
                                let k = names[*o as usize].split(' ').next().unwrap().to_string();
                                *caps_from.entry(k).or_default() += 1;
                            }
                            if mine[*o as usize] {
                                let k = names[*player as usize].split(' ').next().unwrap().to_string();
                                *lost_to.entry(k).or_default() += 1;
                                lost[bin] += 1;
                                lost_total += 1;
                                lost_this_tick[*o as usize] += 1;
                                if last_cap[i].is_some_and(|(lt, who)| who == *o && t - lt <= 30) {
                                    lost_fresh += 1;
                                }
                                last_lost_by[*o as usize][i] = Some(t);
                                let g = pre.cells[i].troops;
                                let b = match g {
                                    0..=2 => 0,
                                    3..=5 => 1,
                                    6..=10 => 2,
                                    11..=20 => 3,
                                    21..=50 => 4,
                                    51..=100 => 5,
                                    101..=500 => 6,
                                    _ => 7,
                                };
                                lost_garrison_hist[b] += 1;
                                *lost_by_attacker_dh
                                    .entry(pre.height(from) - pre.height(*at))
                                    .or_default() += 1;
                                *lost_def_height.entry(pre.height(*at)).or_default() += 1;
                            }
                        }
                        last_cap[i] = Some((t, *player));
                    }
                    Event::Repelled { attacker, .. } if mine[*attacker as usize] => {
                        repelled[bin] += 1;
                    }
                    _ => {}
                }
            }
            for p in 0..names.len() {
                if !mine[p] || st.eliminated[p] {
                    continue;
                }
                if t % 100 == 0 {
                    for h in st.board.hexes() {
                        if st.cell(h).unwrap().owner == Some(p as PlayerId) {
                            *own_height.entry(st.height(h)).or_default() += 1;
                        }
                    }
                }
                if t < 300 {
                    continue;
                }
                if last_gather[p].is_some_and(|g| t - g < 20) {
                    loss_after_gather += lost_this_tick[p];
                    gather_windows += 1;
                } else {
                    loss_base += lost_this_tick[p];
                    base_ticks += 1;
                }
            }
        }
    }
    let s = seats as f64;
    println!("seats {seats}");
    println!("bin(250t)  cap_neutral cap_enemy  lost  gathers repelled internal noop  (per seat)");
    for b in 0..BINS {
        println!(
            "{:>5}  {:>8.1} {:>8.1} {:>8.1} {:>7.1} {:>7.1} {:>7.1} {:>7.1}",
            b * 250,
            cap_neutral[b] as f64 / s,
            cap_enemy[b] as f64 / s,
            lost[b] as f64 / s,
            gathers[b] as f64 / s,
            repelled[b] as f64 / s,
            internal[b] as f64 / s,
            noops[b] as f64 / s
        );
    }
    println!(
        "lost hexes captured by us within 30 ticks before: {:.1}%",
        100.0 * lost_fresh as f64 / lost_total.max(1) as f64
    );
    println!(
        "enemy captures that retake a hex we lost within 30 ticks: {:.1}%",
        100.0 * recaps as f64 / caps_enemy_total.max(1) as f64
    );
    println!("garrison of lost hexes [0-2,3-5,6-10,11-20,21-50,51-100,101-500,500+]: {:?}", lost_garrison_hist);
    println!("lost hexes by attacker_h - our_h: {:?}", lost_by_attacker_dh);
    println!("lost hexes by our height: {:?}", lost_def_height);
    println!("own land height samples: {:?}", own_height);
    println!(
        "loss rate per tick: within 20t after own gather {:.3} ({} ticks), otherwise {:.3}",
        loss_after_gather as f64 / gather_windows.max(1) as f64,
        gather_windows,
        loss_base as f64 / base_ticks.max(1) as f64
    );
    println!("hexes lost to: {:?}", lost_to);
    println!("enemy hexes captured from: {:?}", caps_from);
    println!(
        "source left on frontier after capture: {} times, avg troops left {:.1}",
        src_left_frontier.0,
        src_left_frontier.1 as f64 / src_left_frontier.0.max(1) as f64
    );
}
