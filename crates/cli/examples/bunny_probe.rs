//! Plays one 8-seat game and prints, per seat at fixed ticks, land, troops on the
//! enemy-facing border vs the interior, the biggest stack and action mix so far.
//! usage: bunny_probe --bots a.json,champ,relay,... --seed S

use hexarena_arena::{seat_names, settings};
use hexarena_engine::State;
use hexarena_protocol::*;
use hexarena_sdk::{brains, style, Bot, BotParams, Style};

fn genome(path: &str) -> Style {
    if let Some(s) = style(path) {
        return s;
    }
    if path == "champ" {
        let l = hexarena_league::League::load(std::path::Path::new("/workspace/data/league")).unwrap();
        let g = l.champion();
        return Style { id: "champ".into(), name: "champ".into(), blurb: String::new(), params: g.params.clone(), program: None };
    }
    let p: BotParams = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let id = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().to_string();
    Style { id: id.clone(), name: id, blurb: String::new(), params: p, program: None }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bots = a.iter().position(|x| x == "--bots").map(|i| a[i + 1].clone()).unwrap();
    let seed: u64 = a.iter().position(|x| x == "--seed").map(|i| a[i + 1].parse().unwrap()).unwrap_or(1);
    let styles: Vec<Style> = bots.split(',').map(genome).collect();
    let mut st = State::new(settings(styles.len() as u16, seed), seat_names(&styles)).unwrap();
    let mut bs: Vec<Box<dyn Bot>> = styles.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
    let n = styles.len();
    let mut caps = vec![0u32; n];
    let mut builds = vec![0u32; n];
    let mut moves_own = vec![0u32; n];
    let snaps = [300u32, 600, 1000, 1500, 2000, 2500, 2999];
    while !st.is_over() {
        let cmds: TickCommands = (0..n as PlayerId)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| (p, bs[p as usize].act(&st.view(p))))
            .collect();
        for (p, c) in &cmds {
            if let Action::Move { to, .. } = c.action {
                if st.cell(to).unwrap().owner == Some(*p) {
                    moves_own[*p as usize] += 1;
                }
            }
        }
        st.step(&cmds);
        for e in &st.last_events {
            match e {
                Event::Captured { player, .. } => caps[*player as usize] += 1,
                Event::Built { player, .. } => builds[*player as usize] += 1,
                _ => {}
            }
        }
        if snaps.contains(&st.tick) {
            println!("-- tick {}", st.tick);
            for p in 0..n as PlayerId {
                let (mut land, mut border, mut inner, mut bt, mut it, mut max) = (0, 0, 0, 0u64, 0u64, 0u32);
                for h in st.board.hexes() {
                    let c = st.cell(h).unwrap();
                    if c.owner != Some(p) {
                        continue;
                    }
                    land += 1;
                    max = max.max(c.troops);
                    let hot = h.neighbors().iter().any(|nb| st.cell(*nb).is_some_and(|x| x.owner.is_some_and(|o| o != p)));
                    if hot {
                        border += 1;
                        bt += c.troops as u64;
                    } else {
                        inner += 1;
                        it += c.troops as u64;
                    }
                }
                println!(
                    "{:>14} land {:4} border {:3} ({:7}/hex) inner {:3} ({:6}/hex) max {:7} caps {:4} own-moves {:4} builds {:3} res {}",
                    styles[p as usize].id,
                    land,
                    border,
                    bt / border.max(1) as u64,
                    inner,
                    it / inner.max(1) as u64,
                    max,
                    caps[p as usize],
                    moves_own[p as usize],
                    builds[p as usize],
                    st.resources[p as usize]
                );
            }
        }
    }
    let r = st.result.clone().unwrap();
    for pl in &r.placements {
        println!("{} place {} land {}", styles[pl.player as usize].id, pl.place_start, pl.land);
    }
}
