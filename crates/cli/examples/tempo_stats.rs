//! Replay analysis: how each seat spends its one action a tick.
//! usage: cargo run --release --example tempo_stats -- r1.json [r2.json ...]

use hexarena_engine::State;
use hexarena_protocol::*;
use std::collections::BTreeMap;

#[derive(Default, Clone)]
struct Acc {
    games: u32,
    noop: u64,
    cap_neutral: u64,
    cap_enemy: u64,
    repelled: u64,
    move_own: u64,
    gather: u64,
    build: u64,
    rejected: u64,
    alive_ticks: u64,
    surrendered_gain: u64,
    lost_hexes: u64,
    sent_on_capture: u64,
    survivors_on_capture: u64,
    defenders_on_capture: u64,
    // ticks alive with no capture
    no_capture_ticks: u64,
    no_capture_after500: u64,
    alive_after500: u64,
    interior_troops: u64,
    border_troops: u64,
    samples: u64,
    land_at: BTreeMap<u32, u64>,
    troops_at: BTreeMap<u32, u64>,
    place_sum: f64,
    gather_then_cap5: u64,
    max_border_stack: u64,
    affordable_any: u64,
    affordable_samples: u64,
    lost_bucket: [u64; 6],
    recaptures: u64,
    first_contact: u64,
    contact_games: u64,
    gather_sources: u64,
    phase: [[u64; 5]; 4],
}

fn bucket(t: u32) -> usize {
    match t {
        0..=9 => 0,
        10..=29 => 1,
        30..=99 => 2,
        100..=299 => 3,
        300..=999 => 4,
        _ => 5,
    }
}

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    let mut by_name: BTreeMap<String, Acc> = BTreeMap::new();
    let checkpoints = [25u32, 50, 75, 100, 150, 200, 300, 500, 750, 1000, 1500, 2000, 2500, 2999];
    for f in &files {
        let r: Replay = serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap();
        let names: Vec<String> = r.players.iter().map(|p| p.name.clone()).collect();
        let base = |n: &str| {
            let mut s = n.to_string();
            if let Some(i) = s.rfind(' ') {
                if s[i + 1..].parse::<u32>().is_ok() {
                    s.truncate(i);
                }
            }
            s
        };
        let mut st = State::new(r.settings(), names.clone()).unwrap();
        let n = names.len();
        let mut accs: Vec<Acc> = vec![Acc::default(); n];
        let mut last_gather: Vec<Option<u32>> = vec![None; n];
        let mut lost_by: Vec<Option<PlayerId>> = vec![None; st.board.slots()];
        let mut lost_at: Vec<u32> = vec![0; st.board.slots()];
        let mut first_contact: Vec<Option<u32>> = vec![None; n];
        for (t, cmds) in r.commands.iter().enumerate() {
            let tick = t as u32;
            // sample troop distribution before acting
            if tick % 10 == 0 {
                let mut interior = vec![0u64; n];
                let mut border = vec![0u64; n];
                let mut maxb = vec![0u64; n];
                let mut afford = vec![false; n];
                for h in st.board.hexes() {
                    let c = st.cell(h).unwrap();
                    let Some(o) = c.owner else { continue };
                    let mut is_border = false;
                    for nb in h.neighbors() {
                        let Some(i) = st.board.index(nb) else { continue };
                        let passable = st.terrain[i].is_some_and(|t| t.passable());
                        if passable && st.cells[i].owner != Some(o) {
                            is_border = true;
                            let d = st.defense_milli(nb, st.height(h)) as u64;
                            let loss = st.cells[i].troops as u64 * d * 530 / 470 / 1000;
                            if c.troops as u64 > 2 * loss + 1 {
                                afford[o as usize] = true;
                            }
                        }
                    }
                    let tr = c.troops.saturating_sub(1) as u64;
                    if is_border {
                        border[o as usize] += tr;
                        maxb[o as usize] = maxb[o as usize].max(tr);
                    } else {
                        interior[o as usize] += tr;
                    }
                }
                for p in 0..n {
                    if st.eliminated[p] {
                        continue;
                    }
                    accs[p].interior_troops += interior[p];
                    accs[p].border_troops += border[p];
                    accs[p].max_border_stack += maxb[p];
                    accs[p].samples += 1;
                    accs[p].affordable_samples += 1;
                    accs[p].affordable_any += afford[p] as u64;
                }
            }
            let alive: Vec<bool> = st.eliminated.iter().map(|e| !e).collect();
            let pre = st.clone();
            st.step(cmds);
            let mut captured = vec![false; n];
            for e in &st.last_events {
                match e {
                    Event::Captured { player, previous_owner, troops_left, at, .. } => {
                        let a = &mut accs[*player as usize];
                        let i = st.board.index(*at).unwrap();
                        if lost_by[i] == Some(*player) && tick - lost_at[i] <= 100 {
                            a.recaptures += 1;
                        }
                        if previous_owner.is_some() {
                            a.cap_enemy += 1;
                            let po = previous_owner.unwrap() as usize;
                            lost_by[i] = Some(po as PlayerId);
                            lost_at[i] = tick;
                            accs[po].lost_hexes += 1;
                            accs[po].lost_bucket[bucket(pre.cell(*at).unwrap().troops)] += 1;
                            if first_contact[*player as usize].is_none() {
                                first_contact[*player as usize] = Some(tick);
                            }
                        } else {
                            a.cap_neutral += 1;
                        }
                        captured[*player as usize] = true;
                        accs[*player as usize].survivors_on_capture += *troops_left as u64;
                        accs[*player as usize].defenders_on_capture +=
                            pre.cell(*at).unwrap().troops as u64;
                    }
                    Event::Repelled { attacker, .. } => accs[*attacker as usize].repelled += 1,
                    Event::Surrendered { capturer, from, hexes } => {
                        accs[*capturer as usize].surrendered_gain += hexes.len() as u64;
                        accs[*from as usize].lost_hexes += hexes.len() as u64;
                    }
                    Event::Rejected { player, .. } => accs[*player as usize].rejected += 1,
                    Event::Gathered { player, sources, .. } => {
                        accs[*player as usize].gather_sources += *sources as u64
                    }
                    _ => {}
                }
            }
            for (p, cmd) in cmds {
                let p = *p as usize;
                if !alive[p] {
                    continue;
                }
                match &cmd.action {
                    Action::NoOp => accs[p].noop += 1,
                    Action::Gather { .. } => {
                        accs[p].gather += 1;
                        last_gather[p] = Some(tick);
                    }
                    Action::Build { .. } => accs[p].build += 1,
                    Action::Move { from, to, troops } => {
                        if pre.cell(*to).unwrap().owner == Some(p as PlayerId) {
                            accs[p].move_own += 1;
                        } else if captured[p] {
                            accs[p].sent_on_capture += *troops as u64;
                        }
                        let _ = from;
                    }
                }
            }
            let ph = if tick < 60 { 0 } else if tick < 150 { 1 } else if tick < 500 { 2 } else { 3 };
            for (p, cmd) in cmds {
                let p = *p as usize;
                if !alive[p] { continue; }
                let rep = st.last_events.iter().any(|e| matches!(e, Event::Repelled { attacker, .. } if *attacker as usize == p));
                let k = if captured[p] { 1 } else if rep { 2 } else if matches!(cmd.action, Action::Gather { .. }) { 3 } else { 4 };
                accs[p].phase[ph][0] += 1;
                accs[p].phase[ph][k] += 1;
            }
            for p in 0..n {
                if !alive[p] {
                    continue;
                }
                accs[p].alive_ticks += 1;
                if !captured[p] {
                    accs[p].no_capture_ticks += 1;
                    if tick >= 500 {
                        accs[p].no_capture_after500 += 1;
                    }
                } else if let Some(g) = last_gather[p] {
                    if tick - g <= 5 {
                        accs[p].gather_then_cap5 += 1;
                        last_gather[p] = None;
                    }
                }
                if tick >= 500 {
                    accs[p].alive_after500 += 1;
                }
            }
            if checkpoints.contains(&tick) {
                let counts = st.hex_counts();
                let mut troops = vec![0u64; n];
                for c in &st.cells {
                    if let Some(o) = c.owner {
                        troops[o as usize] += c.troops as u64;
                    }
                }
                for p in 0..n {
                    *accs[p].land_at.entry(tick).or_default() += counts[p] as u64;
                    *accs[p].troops_at.entry(tick).or_default() += troops[p];
                }
            }
        }
        // Pad checkpoints past the end with the final value.
        let counts = st.hex_counts();
        for p in 0..n {
            for &c in &checkpoints {
                if c as usize >= r.commands.len() {
                    *accs[p].land_at.entry(c).or_default() += counts[p] as u64;
                    accs[p].troops_at.entry(c).or_default();
                }
            }
        }
        let res = st.result.clone().unwrap_or_else(|| st.final_result());
        for p in 0..n {
            accs[p].games = 1;
            if let Some(t) = first_contact[p] {
                accs[p].first_contact = t as u64;
                accs[p].contact_games = 1;
            }
            accs[p].place_sum = res.place_f64(p as PlayerId);
            let key = base(&names[p]);
            let e = by_name.entry(key).or_default();
            let a = &accs[p];
            e.games += a.games;
            e.noop += a.noop;
            e.cap_neutral += a.cap_neutral;
            e.cap_enemy += a.cap_enemy;
            e.repelled += a.repelled;
            e.move_own += a.move_own;
            e.gather += a.gather;
            e.build += a.build;
            e.rejected += a.rejected;
            e.alive_ticks += a.alive_ticks;
            e.surrendered_gain += a.surrendered_gain;
            e.lost_hexes += a.lost_hexes;
            e.sent_on_capture += a.sent_on_capture;
            e.survivors_on_capture += a.survivors_on_capture;
            e.defenders_on_capture += a.defenders_on_capture;
            e.no_capture_ticks += a.no_capture_ticks;
            e.no_capture_after500 += a.no_capture_after500;
            e.alive_after500 += a.alive_after500;
            e.interior_troops += a.interior_troops;
            e.border_troops += a.border_troops;
            e.max_border_stack += a.max_border_stack;
            e.samples += a.samples;
            e.place_sum += a.place_sum;
            e.gather_then_cap5 += a.gather_then_cap5;
            e.affordable_any += a.affordable_any;
            e.affordable_samples += a.affordable_samples;
            for i in 0..6 {
                e.lost_bucket[i] += a.lost_bucket[i];
            }
            e.recaptures += a.recaptures;
            e.first_contact += a.first_contact;
            e.contact_games += a.contact_games;
            e.gather_sources += a.gather_sources;
            for i in 0..4 { for j in 0..5 { e.phase[i][j] += a.phase[i][j]; } }
            for (k, v) in &a.land_at {
                *e.land_at.entry(*k).or_default() += v;
            }
            for (k, v) in &a.troops_at {
                *e.troops_at.entry(*k).or_default() += v;
            }
        }
        println!(
            "{f}: ticks {} passable {} | {}",
            r.commands.len(),
            st.terrain.iter().flatten().filter(|t| t.passable()).count(),
            res.placements
                .iter()
                .map(|pl| format!("{}:{}", base(&names[pl.player as usize]), pl.land))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    for (name, a) in &by_name {
        let g = a.games as f64;
        let at = a.alive_ticks.max(1) as f64;
        let caps = (a.cap_enemy + a.cap_neutral).max(1) as f64;
        println!("\n== {name} ({} seats, avg place {:.2})", a.games, a.place_sum / g);
        println!(
            "  alive ticks/seat {:.0}; per alive tick: noop {:.1}% capN {:.1}% capE {:.1}% repelled {:.1}% moveOwn {:.1}% gather {:.1}% build {:.1}% rejected {:.1}%",
            at / g,
            100.0 * a.noop as f64 / at,
            100.0 * a.cap_neutral as f64 / at,
            100.0 * a.cap_enemy as f64 / at,
            100.0 * a.repelled as f64 / at,
            100.0 * a.move_own as f64 / at,
            100.0 * a.gather as f64 / at,
            100.0 * a.build as f64 / at,
            100.0 * a.rejected as f64 / at,
        );
        println!(
            "  no-capture ticks {:.1}% (after t500: {:.1}%), surrender gain/seat {:.1}, lost hexes/seat {:.1}, gather->capture<=5t {:.1}% of gathers",
            100.0 * a.no_capture_ticks as f64 / at,
            100.0 * a.no_capture_after500 as f64 / a.alive_after500.max(1) as f64,
            a.surrendered_gain as f64 / g,
            a.lost_hexes as f64 / g,
            100.0 * a.gather_then_cap5 as f64 / a.gather.max(1) as f64,
        );
        println!(
            "  per capture: sent {:.1}, defenders {:.1}, survivors {:.1} (spent {:.1})",
            a.sent_on_capture as f64 / caps,
            a.defenders_on_capture as f64 / caps,
            a.survivors_on_capture as f64 / caps,
            (a.sent_on_capture as f64 - a.survivors_on_capture as f64) / caps
        );
        let s = a.samples.max(1) as f64;
        println!(
            "  avg spare troops: interior {:.0}, border {:.0}, biggest border stack {:.0}; samples with a 2x-affordable capture {:.1}%",
            a.interior_troops as f64 / s,
            a.border_troops as f64 / s,
            a.max_border_stack as f64 / s,
            100.0 * a.affordable_any as f64 / a.affordable_samples.max(1) as f64
        );
        let lb: u64 = a.lost_bucket.iter().sum::<u64>().max(1);
        println!(
            "  hexes lost to captures by defender troops [0-9,10-29,30-99,100-299,300-999,1000+]: {:?}%; recaptures (hex lost to us <=100t ago) {:.1}% of our captures; first enemy capture t{:.0}; avg gather sources {:.0}",
            a.lost_bucket.iter().map(|b| (100 * b / lb) as u32).collect::<Vec<_>>(),
            100.0 * a.recaptures as f64 / caps,
            a.first_contact as f64 / a.contact_games.max(1) as f64,
            a.gather_sources as f64 / a.gather.max(1) as f64,
        );
        println!("  phases t<60, 60-150, 150-500, 500+ (capture/repelled/gather/other %): {}",
            a.phase.iter().map(|x| { let t = x[0].max(1) as f64; format!("[{:.0}/{:.0}/{:.0}/{:.0}]", 100.0*x[1] as f64/t, 100.0*x[2] as f64/t, 100.0*x[3] as f64/t, 100.0*x[4] as f64/t) }).collect::<Vec<_>>().join(" "));
        println!(
            "  land:   {}",
            a.land_at.iter().map(|(k, v)| format!("t{k}={:.0}", *v as f64 / g)).collect::<Vec<_>>().join(" ")
        );
        println!(
            "  troops: {}",
            a.troops_at.iter().map(|(k, v)| format!("t{k}={:.0}", *v as f64 / g)).collect::<Vec<_>>().join(" ")
        );
    }
}
