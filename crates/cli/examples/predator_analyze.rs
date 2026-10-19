//! Replay analysis for conflict studies: who fights whom, eliminations, land curves.
//! usage: cargo run --release -p hexarena-cli --example predator_analyze -- r1.json [r2.json ...]

use hexarena_engine::State;
use hexarena_protocol::*;

fn main() {
    for path in std::env::args().skip(1) {
        let replay: Replay = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        analyze(&path, &replay);
    }
}

fn analyze(path: &str, replay: &Replay) {
    let n = replay.players.len();
    let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
    let mut st = State::new(replay.settings(), names.clone()).unwrap();
    let passable = st.terrain.iter().flatten().filter(|t| t.passable()).count();
    // took[a][v]: hexes a took from v (v == n means neutral); surrendered counted separately.
    let mut took = vec![vec![0u32; n + 1]; n];
    let mut surr = vec![vec![0u32; n]; n];
    let mut repelled = vec![0u32; n];
    let mut repelled_lost = vec![0u64; n];
    let mut kinds = vec![[0u32; 6]; n]; // noop, attack, internal move, gather, build, rejected
    let mut elim_by: Vec<Option<(u32, usize)>> = vec![None; n];
    let mut curve: Vec<Vec<(u32, u32, u64, u32)>> = vec![vec![]; n];
    let mut first_contact: Vec<Option<u32>> = vec![None; n];
    let mut sent_over_loss = vec![(0u64, 0u64); n];
    for (t, cmds) in replay.commands.iter().enumerate() {
        for (p, c) in cmds {
            let p = *p as usize;
            match &c.action {
                Action::NoOp => kinds[p][0] += 1,
                Action::Move { to, troops, .. } => {
                    if st.cell(*to).and_then(|c| c.owner) == Some(p as PlayerId) {
                        kinds[p][2] += 1
                    } else {
                        kinds[p][1] += 1;
                        let def = st.cell(*to).map_or(0, |c| c.troops) as u64;
                        sent_over_loss[p].0 += *troops as u64;
                        sent_over_loss[p].1 += def;
                    }
                }
                Action::Gather { .. } => kinds[p][3] += 1,
                Action::Build { .. } | Action::Demolish { .. } => kinds[p][4] += 1,
            }
        }
        st.step(cmds);
        let mut last_taker: Vec<Option<usize>> = vec![None; n];
        for e in &st.last_events {
            match e {
                Event::Captured { player, previous_owner, .. } => {
                    let a = *player as usize;
                    let v = previous_owner.map_or(n, |o| o as usize);
                    took[a][v] += 1;
                    if v < n {
                        last_taker[v] = Some(a);
                        first_contact[a].get_or_insert(t as u32);
                        first_contact[v].get_or_insert(t as u32);
                    }
                }
                Event::Surrendered { capturer, from, hexes } => {
                    surr[*capturer as usize][*from as usize] += hexes.len() as u32;
                    last_taker[*from as usize] = Some(*capturer as usize);
                }
                Event::Repelled { attacker, attackers_lost, .. } => {
                    repelled[*attacker as usize] += 1;
                    repelled_lost[*attacker as usize] += *attackers_lost as u64;
                }
                Event::Rejected { player, .. } => kinds[*player as usize][5] += 1,
                Event::Eliminated { player } => {
                    let v = *player as usize;
                    elim_by[v] = Some((t as u32, last_taker[v].unwrap_or(99)));
                }
                _ => {}
            }
        }
        if t % 250 == 249 || t + 1 == replay.commands.len() {
            let land = st.hex_counts();
            let mut troops = vec![0u64; n];
            let mut maxs = vec![0u32; n];
            for c in &st.cells {
                if let Some(o) = c.owner {
                    troops[o as usize] += c.troops as u64;
                    maxs[o as usize] = maxs[o as usize].max(c.troops);
                }
            }
            for p in 0..n {
                curve[p].push((t as u32 + 1, land[p], troops[p], maxs[p]));
            }
        }
    }
    let res = &replay.result;
    println!("=== {path}  seed {} final_tick {} passable {passable}", replay.match_seed, res.final_tick);
    for pl in &res.placements {
        let p = pl.player as usize;
        let k = kinds[p];
        println!(
            "#{} p{} {:<12} land {:>4} elim {:?} by {:?} | acts noop {} atk {} mv {} gath {} build {} rej {} | repelled {} (lost {}) | sent/def {:.2} | first_contact {:?}",
            pl.place_start, p, names[p], pl.land, elim_by[p].map(|x| x.0), elim_by[p].map(|x| x.1),
            k[0], k[1], k[2], k[3], k[4], k[5], repelled[p], repelled_lost[p],
            sent_over_loss[p].0 as f64 / sent_over_loss[p].1.max(1) as f64, first_contact[p]
        );
        let tk: Vec<String> = (0..=n)
            .filter(|&v| took[p][v] > 0 || (v < n && surr[p][v] > 0))
            .map(|v| {
                if v == n {
                    format!("neutral:{}", took[p][v])
                } else {
                    format!("p{v}:{}+{}s", took[p][v], surr[p][v])
                }
            })
            .collect();
        println!("     took {}", tk.join(" "));
        let cv: Vec<String> = curve[p].iter().map(|(t, l, tr, m)| format!("{t}:{l}/{}k/{}", tr / 1000, m)).collect();
        println!("     curve {}", cv.join(" "));
    }
}
