//! How games end with a longer cutoff: plays roster CPUs with `max_ticks` raised and
//! prints when (and whether) each game ended and the land curve of the leaders.
//!
//! `cargo run --release -p hexarena-cli --example long_games -- 10000 4 ridgeline,ridgeline,predator,relay,ridgeline,predator,relay,expander`

use hexarena_arena::{seat_names, settings};
use hexarena_engine::State;
use hexarena_protocol::PlayerId;
use hexarena_sdk::{brains, style, Bot};

fn main() {
    let mut args = std::env::args().skip(1);
    let max_ticks: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let games: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2);
    let ids = args.next().unwrap_or_else(|| "ridgeline,ridgeline,predator,relay,ridgeline,predator,relay,expander".into());
    let styles: Vec<_> = ids.split(',').map(|id| style(id).unwrap_or_else(|| panic!("unknown roster bot {id}"))).collect();
    for g in 0..games {
        let seed = 9_000 + g;
        let mut s = settings(styles.len() as u16, seed);
        s.rules.max_ticks = max_ticks;
        let mut st = State::new(s, seat_names(&styles)).unwrap();
        let mut bots: Vec<Box<dyn Bot>> = styles.iter().map(|s| brains::build(s.clone()).unwrap()).collect();
        let mut marks = vec![];
        while !st.is_over() && st.tick < max_ticks {
            let cmds: Vec<_> = (0..styles.len() as PlayerId)
                .filter(|&p| !st.eliminated[p as usize])
                .map(|p| (p, bots[p as usize].act(&st.view(p))))
                .collect();
            st.step(&cmds);
            if st.tick % 1000 == 0 {
                let alive = st.eliminated.iter().filter(|e| !**e).count();
                let mut land: Vec<u32> = st.hex_counts().to_vec();
                land.sort_unstable_by(|a, b| b.cmp(a));
                let n = styles.len();
                let (mut troops, mut peak, mut moves) = (vec![0u64; n], vec![0u32; n], 0);
                for h in st.board.hexes() {
                    let c = st.cell(h).unwrap();
                    if let Some(o) = c.owner {
                        troops[o as usize] += c.troops as u64;
                        peak[o as usize] = peak[o as usize].max(c.troops);
                    }
                }
                for (p, cmd) in &cmds {
                    if !st.eliminated[*p as usize] && !matches!(cmd.action, hexarena_protocol::Action::NoOp) {
                        moves += 1;
                    }
                }
                let alive_stats: Vec<String> = (0..n)
                    .filter(|&p| !st.eliminated[p])
                    .map(|p| format!("{}:{}h/{}t/max{}", st.names[p], st.hex_counts()[p], troops[p], peak[p]))
                    .collect();
                marks.push(format!("t{} alive {alive} acting {moves} {:?} | {}", st.tick, &land[..3], alive_stats.join(" ")));
            }
        }
        let winner = st.conqueror().map(|p| st.names[p as usize].clone());
        println!("game {g}: ended at tick {} winner {:?}", st.tick, winner);
        for m in marks {
            println!("   {m}");
        }
    }
}
