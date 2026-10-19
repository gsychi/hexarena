//! Land per seat at checkpoints through one lobby, to see when seats win or lose ground.
//!
//! cargo run --release --example apex_trace -- SEED a.json,b.json,...

use hexarena_arena::{seat_names, settings};
use hexarena_engine::State;
use hexarena_protocol::{Building, PlayerId, TickCommands};
use hexarena_sdk::{brains, BotParams, Style};

fn load(path: &str) -> Style {
    if let Some(s) = hexarena_sdk::style(path) {
        return s;
    }
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).expect("read params")).expect("json");
    let inner = v.get("params").cloned().unwrap_or(v);
    let params: BotParams = serde_json::from_value(inner).expect("params");
    Style { id: path.into(), name: path.into(), blurb: String::new(), params, program: None }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a[1].parse().unwrap();
    let styles: Vec<Style> = a[2].split(',').map(load).collect();
    let mut st = State::new(settings(styles.len() as u16, seed), seat_names(&styles)).unwrap();
    let mut bots: Vec<_> = styles.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
    let checkpoints: Vec<u32> = match std::env::var("CHECK") {
        Ok(v) => {
            let p: Vec<u32> = v.split(',').map(|x| x.parse().unwrap()).collect();
            (p[0]..p[1]).step_by(p[2] as usize).collect()
        }
        Err(_) => vec![50, 100, 200, 400, 700, 1000, 1500, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000, 9500],
    };
    let mut actions = vec![[0u32; 4]; styles.len()];
    let print = |st: &State, actions: &Vec<[u32; 4]>| {
        let land = st.hex_counts();
        let mut troops = vec![0u64; styles.len()];
        let mut builds = vec![0u32; styles.len()];
        let mut walls = vec![0u32; styles.len()];
        for c in &st.cells {
            if let Some(o) = c.owner {
                troops[o as usize] += c.troops as u64;
                builds[o as usize] += c.building.is_some() as u32;
                walls[o as usize] += (c.building == Some(Building::Wall)) as u32;
            }
        }
        let cols: Vec<String> = (0..styles.len())
            .map(|p| {
                if st.eliminated[p] {
                    format!("{:>16}", format!("dead@{}", st.eliminated_at[p].unwrap()))
                } else {
                    format!("{:>4}/{:>7}/{:>3}", land[p], troops[p], builds[p])
                }
            })
            .collect();
        println!("t{:>5} {}", st.tick, cols.join(" "));
        let _ = (walls, actions);
    };
    let n = styles.len();
    let (mut cap, mut neu, mut lost, mut rep, mut sur) = (vec![0u32; n], vec![0u32; n], vec![0u32; n], vec![0u32; n], vec![0u32; n]);
    let detail = std::env::var("DETAIL").is_ok();
    let watch: Option<usize> = std::env::var("WATCH").ok().map(|v| v.parse().unwrap());
    println!("seats: {}", styles.iter().map(|s| s.id.clone()).collect::<Vec<_>>().join(" | "));
    while !st.is_over() {
        let cmds: TickCommands = (0..styles.len() as PlayerId)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| {
                let cmd = bots[p as usize].act(&st.view(p));
                let k = match cmd.action {
                    hexarena_protocol::Action::NoOp => 0,
                    hexarena_protocol::Action::Move { .. } => 1,
                    hexarena_protocol::Action::Gather { .. } => 2,
                    hexarena_protocol::Action::Build { .. } => 3,
                };
                actions[p as usize][k] += 1;
                (p, cmd)
            })
            .collect();
        st.step(&cmds);
        for e in &st.last_events {
            match e {
                hexarena_protocol::Event::Captured { player, previous_owner, .. } => {
                    if previous_owner.is_some() { cap[*player as usize] += 1; }
                    else { neu[*player as usize] += 1; }
                    if let Some(o) = previous_owner { lost[*o as usize] += 1; }
                }
                hexarena_protocol::Event::Repelled { attacker, .. } => rep[*attacker as usize] += 1,
                hexarena_protocol::Event::Surrendered { capturer, from, hexes } => {
                    sur[*capturer as usize] += hexes.len() as u32;
                    lost[*from as usize] += hexes.len() as u32;
                }
                _ => {}
            }
        }
        if let Some(w) = watch {
            if checkpoints.contains(&st.tick) && !st.eliminated[w] {
                let mut parts = vec![];
                for (i, c) in st.cells.iter().enumerate() {
                    if c.owner != Some(w as PlayerId) { continue; }
                    let h = st.board.hex_at(i);
                    let foes = h.neighbors().iter().filter(|n| st.cell(**n).is_some_and(|x| x.owner.is_some_and(|y| y != w as PlayerId))).count();
                    let edge = h.neighbors().iter().filter(|n| !hexarena_engine::mapgen::passable(&st.board, &st.terrain, **n)).count();
                    parts.push(format!("({},{}):{}{} f{} e{}", h.q, h.r, c.troops, c.building.map(|b| format!("{:?}", b)[..1].to_string()).unwrap_or_default(), foes, edge));
                }
                println!("   watch s{w}: {}", parts.join(" "));
            }
        }
        if checkpoints.contains(&st.tick) {
            print(&st, &actions);
            if detail {
                let mut maxs = vec![0u32; styles.len()];
                let mut border = vec![0u64; styles.len()];
                for (i, c) in st.cells.iter().enumerate() {
                    let Some(o) = c.owner else { continue };
                    maxs[o as usize] = maxs[o as usize].max(c.troops);
                    let h = st.board.hex_at(i);
                    if h.neighbors().iter().any(|n| st.cell(*n).is_some_and(|x| x.owner.is_some_and(|y| y != o))) {
                        border[o as usize] += c.troops as u64;
                    }
                }
                let cols: Vec<String> = (0..styles.len()).filter(|&p| !st.eliminated[p]).map(|p| format!("s{p}: cap{} neu{} lost{} rep{} sur{} max{} border{}", cap[p], neu[p], lost[p], rep[p], sur[p], maxs[p], border[p])).collect();
                println!("        {}", cols.join(" | "));
                cap.iter_mut().for_each(|x| *x = 0); neu.iter_mut().for_each(|x| *x = 0); lost.iter_mut().for_each(|x| *x = 0); rep.iter_mut().for_each(|x| *x = 0); sur.iter_mut().for_each(|x| *x = 0);
            }
        }
    }
    print(&st, &actions);
    let r = st.result.clone().unwrap();
    for pl in &r.placements {
        println!(
            "place {}-{}: seat {} {} land {} actions noop/move/gather/build {:?}",
            pl.place_start, pl.place_end, pl.player, styles[pl.player as usize].id, pl.land, actions[pl.player as usize]
        );
    }
}
