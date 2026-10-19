//! Geometry-focused replay analysis: where each seat expands, what it pays per capture
//! by height, how long its border is, whether it encloses anything and whether its
//! buildings pay off.
//!
//! cargo run --release -p hexarena-cli --example terrain_stats -- r1.json [r2.json ...]

use hexarena_engine::State;
use hexarena_protocol::*;
use std::collections::BTreeMap;

#[derive(Default, Clone)]
struct Seat {
    name: String,
    moves_out: u64,
    captures: u64,
    captures_enemy: u64,
    repelled: u64,
    troops_sent_cap: u64,
    troops_lost_repel: u64,
    gathers: u64,
    builds: u64,
    noop: u64,
    rejected: u64,
    internal_moves: u64,
    surrendered_gain: u64,
    surrendered_events: u64,
    lost_to_enemy: u64,
    lost_surrendered: u64,
    /// captures by (from_h - to_h)
    by_dh: BTreeMap<i8, (u64, u64)>,
    /// captures by target height
    by_to_h: BTreeMap<i8, u64>,
    samples: Vec<(u32, Snap)>,
    neutral_left_tick: Option<u32>,
}

#[derive(Default, Clone, Copy)]
struct Snap {
    land: u32,
    troops: u64,
    frontier: u32,
    edges_open: u32,
    edges_enemy: u32,
    edges_neutral: u32,
    edges_impass: u32,
    mean_h_x100: i32,
    max_stack: u32,
    frontier_troops: u64,
}

fn snap(st: &State, p: PlayerId) -> Snap {
    let mut s = Snap::default();
    let mut hsum = 0i32;
    for h in st.board.hexes() {
        let c = st.cell(h).unwrap();
        if c.owner != Some(p) {
            continue;
        }
        s.land += 1;
        s.troops += c.troops as u64;
        s.max_stack = s.max_stack.max(c.troops);
        hsum += st.height(h) as i32;
        let mut front = false;
        for nb in h.neighbors() {
            let Some(i) = st.board.index(nb) else {
                s.edges_impass += 1;
                continue;
            };
            if !st.terrain[i].is_some_and(|t| t.passable()) {
                s.edges_impass += 1;
                continue;
            }
            match st.cells[i].owner {
                Some(o) if o == p => {}
                Some(_) => {
                    s.edges_enemy += 1;
                    s.edges_open += 1;
                    front = true;
                }
                None => {
                    s.edges_neutral += 1;
                    s.edges_open += 1;
                    front = true;
                }
            }
        }
        if front {
            s.frontier += 1;
            s.frontier_troops += c.troops as u64;
        }
    }
    s.mean_h_x100 = if s.land > 0 { hsum * 100 / s.land as i32 } else { 0 };
    s
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let sample_ticks = [100u32, 250, 500, 750, 1000, 1500, 2000, 2500, 2999];
    let mut agg: BTreeMap<String, Vec<Seat>> = BTreeMap::new();
    let mut map_h = BTreeMap::<i8, u64>::new();
    for path in &paths {
        let replay: Replay = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let mut st = State::new(replay.settings(), names.clone()).unwrap();
        for h in st.board.hexes() {
            if st.terrain[st.board.index(h).unwrap()].is_some_and(|t| t.passable()) {
                *map_h.entry(st.height(h)).or_default() += 1;
            }
        }
        let n = names.len();
        let mut seats: Vec<Seat> = names
            .iter()
            .map(|nm| Seat { name: nm.clone(), ..Default::default() })
            .collect();
        let mut neutral_gone: Option<u32> = None;
        for cmds in &replay.commands {
            let pre = st.clone();
            for (p, cmd) in cmds {
                let s = &mut seats[*p as usize];
                match &cmd.action {
                    Action::NoOp => s.noop += 1,
                    Action::Gather { .. } => s.gathers += 1,
                    Action::Build { .. } => s.builds += 1,
                    Action::Move { from, to, troops } => {
                        if pre.cell(*to).is_some_and(|c| c.owner == Some(*p)) {
                            s.internal_moves += 1;
                        } else {
                            s.moves_out += 1;
                            let _ = troops;
                            let _ = from;
                        }
                    }
                }
            }
            st.step(cmds);
            // Attribute events.
            let mut last_move: BTreeMap<PlayerId, (Hex, Hex, u32)> = BTreeMap::new();
            for e in &st.last_events {
                match e {
                    Event::Moved { player, from, to, troops } => {
                        last_move.insert(*player, (*from, *to, *troops));
                    }
                    Event::Captured { player, at, previous_owner, .. } => {
                        let s = &mut seats[*player as usize];
                        s.captures += 1;
                        let (from, _, sent) = last_move[player];
                        s.troops_sent_cap += sent as u64;
                        let dh = pre.height(from) - pre.height(*at);
                        let e2 = s.by_dh.entry(dh).or_default();
                        e2.0 += 1;
                        *s.by_to_h.entry(pre.height(*at)).or_default() += 1;
                        if let Some(o) = previous_owner {
                            s.captures_enemy += 1;
                            seats[*o as usize].lost_to_enemy += 1;
                        }
                    }
                    Event::Repelled { attacker, at, attackers_lost } => {
                        let s = &mut seats[*attacker as usize];
                        s.repelled += 1;
                        s.troops_lost_repel += *attackers_lost as u64;
                        let (from, _, _) = last_move[attacker];
                        let dh = pre.height(from) - pre.height(*at);
                        s.by_dh.entry(dh).or_default().1 += 1;
                    }
                    Event::Surrendered { capturer, from, hexes } => {
                        seats[*capturer as usize].surrendered_gain += hexes.len() as u64;
                        seats[*capturer as usize].surrendered_events += 1;
                        seats[*from as usize].lost_surrendered += hexes.len() as u64;
                    }
                    Event::Rejected { player, .. } => seats[*player as usize].rejected += 1,
                    _ => {}
                }
            }
            let t = st.tick - 1;
            if neutral_gone.is_none() {
                let any_neutral = st.board.hexes().iter().any(|&h| {
                    let i = st.board.index(h).unwrap();
                    st.terrain[i].is_some_and(|t| t.passable()) && st.cells[i].owner.is_none()
                });
                if !any_neutral {
                    neutral_gone = Some(t);
                }
            }
            if sample_ticks.contains(&t) || st.is_over() {
                for p in 0..n {
                    seats[p].samples.push((t, snap(&st, p as PlayerId)));
                }
            }
        }
        let res = st.result.clone().unwrap_or_else(|| st.final_result());
        println!("== {path}  final tick {} neutral gone at {:?}", res.final_tick, neutral_gone);
        for pl in &res.placements {
            let s = &seats[pl.player as usize];
            let last = s.samples.last().map(|x| x.1).unwrap_or_default();
            println!(
                "  #{} {:<16} land {:>4} troops {:>6} caps {:>4} (enemy {:>4}) rep {:>3} lost {:>4} surr+{} -{} gath {:>4} build {:>3} noop {:>4} rej {:>3} | front {:>3} edges open {:>3} (enemy {:>3}) impass {:>3}",
                pl.place_start, s.name, pl.land, pl.troops, s.captures, s.captures_enemy, s.repelled,
                s.lost_to_enemy, s.surrendered_gain, s.lost_surrendered, s.gathers, s.builds, s.noop,
                s.rejected, last.frontier, last.edges_open, last.edges_enemy, last.edges_impass
            );
        }
        if std::env::var("PERSEAT").is_ok() {
            for s in &seats {
                let tl: Vec<String> = s.samples.iter().map(|(t, sn)| format!("{}:{}", t, sn.land)).collect();
                println!("    {:<16} {}", s.name, tl.join(" "));
            }
        }
        for s in seats.iter_mut() {
            s.neutral_left_tick = neutral_gone;
        }
        for s in seats {
            let key = s.name.split(' ').take(2).collect::<Vec<_>>().join(" ");
            agg.entry(key).or_default().push(s);
        }
    }
    println!("\n== map height composition (passable): {:?}", map_h);
    for (k, v) in &agg {
        println!("\n### {k}  ({} seats)", v.len());
        let mut by_dh = BTreeMap::<i8, (u64, u64)>::new();
        let mut by_to = BTreeMap::<i8, u64>::new();
        let (mut caps, mut ce, mut rep, mut sent, mut lostr, mut gath, mut noop, mut surr, mut se, mut lost, mut lsurr) =
            (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        for s in v {
            for (d, x) in &s.by_dh {
                let e = by_dh.entry(*d).or_default();
                e.0 += x.0;
                e.1 += x.1;
            }
            for (d, x) in &s.by_to_h {
                *by_to.entry(*d).or_default() += x;
            }
            caps += s.captures;
            ce += s.captures_enemy;
            rep += s.repelled;
            sent += s.troops_sent_cap;
            lostr += s.troops_lost_repel;
            gath += s.gathers;
            noop += s.noop;
            surr += s.surrendered_gain;
            se += s.surrendered_events;
            lost += s.lost_to_enemy;
            lsurr += s.lost_surrendered;
        }
        let m = v.len() as f64;
        println!(
            "  per seat: internal-moves {:.0} attacks {:.0} caps {:.0} enemy-caps {:.0} repelled {:.1} sent/cap {:.1} lost-in-repel {:.0} gathers {:.0} noop {:.0} surrender-gain {:.1} ({:.1} events) hexes-lost {:.0} lost-surr {:.1}",
            v.iter().map(|s| s.internal_moves).sum::<u64>() as f64 / m, v.iter().map(|s| s.moves_out).sum::<u64>() as f64 / m,
            caps as f64 / m, ce as f64 / m, rep as f64 / m, sent as f64 / caps.max(1) as f64,
            lostr as f64 / m, gath as f64 / m, noop as f64 / m, surr as f64 / m, se as f64 / m, lost as f64 / m, lsurr as f64 / m
        );
        println!("  captures by from_h-to_h (caps, repelled): {:?}", by_dh);
        println!("  captures by target height: {:?}", by_to);
        let ticks: Vec<u32> = v[0].samples.iter().map(|x| x.0).collect();
        for (i, t) in ticks.iter().enumerate() {
            let mut a = [0f64; 10];
            let mut cnt = 0.0;
            for s in v {
                if let Some((_, sn)) = s.samples.get(i) {
                    if sn.land == 0 {
                        continue;
                    }
                    cnt += 1.0;
                    a[0] += sn.land as f64;
                    a[1] += sn.troops as f64;
                    a[2] += sn.frontier as f64;
                    a[3] += sn.edges_open as f64;
                    a[4] += sn.edges_enemy as f64;
                    a[5] += sn.edges_impass as f64;
                    a[6] += sn.mean_h_x100 as f64 / 100.0;
                    a[7] += sn.max_stack as f64;
                    a[8] += sn.frontier_troops as f64;
                    a[9] += sn.edges_neutral as f64;
                }
            }
            if cnt == 0.0 {
                continue;
            }
            for x in a.iter_mut() {
                *x /= cnt;
            }
            println!(
                "  t{:>4} alive {:>2} land {:>6.1} troops {:>7.0} frontier {:>5.1} ({:.0}%) open-edges {:>5.1} enemy-edges {:>5.1} neutral-edges {:>5.1} impass-edges {:>5.1} meanH {:+.2} maxstack {:>6.0} front-troops {:>6.0}",
                t, cnt, a[0], a[1], a[2], 100.0 * a[2] / a[0].max(1.0), a[3], a[4], a[9], a[5], a[6], a[7], a[8]
            );
        }
    }
}
