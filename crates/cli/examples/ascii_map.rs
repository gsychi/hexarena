//! Prints the board at given ticks: seat digits (uppercase letters for >= 200 troops),
//! `.` neutral, `#` mountain, `~` water, plus each seat's land and troops.
//!
//! cargo run --release -p hexarena-cli --example ascii_map -- replay.json 250 500

use hexarena_engine::State;
use hexarena_protocol::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let replay: Replay = serde_json::from_slice(&std::fs::read(&args[0]).unwrap()).unwrap();
    let ticks: Vec<u32> = args[1..].iter().map(|t| t.parse().unwrap()).collect();
    let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
    let mut st = State::new(replay.settings(), names.clone()).unwrap();
    for (i, n) in names.iter().enumerate() {
        println!("{i} = {n}");
    }
    for cmds in &replay.commands {
        st.step(cmds);
        if !ticks.contains(&st.tick) {
            continue;
        }
        println!("--- tick {}", st.tick);
        let r = st.board.radius;
        for row in -r..=r {
            let mut line = " ".repeat(row.unsigned_abs() as usize);
            for q in -r..=r {
                let h = Hex::new(q, row);
                if !st.board.contains(h) {
                    continue;
                }
                let i = st.board.index(h).unwrap();
                let t = st.terrain[i].unwrap();
                let c = &st.cells[i];
                let ch = match (t.kind, c.owner) {
                    (TerrainKind::Mountain, _) => '#',
                    (TerrainKind::Water, _) => '~',
                    (_, None) => '.',
                    (_, Some(p)) if c.troops >= 200 => (b'A' + p as u8) as char,
                    (_, Some(p)) => (b'0' + p as u8) as char,
                };
                line.push(ch);
                line.push(' ');
            }
            println!("{line}");
        }
        let counts = st.hex_counts();
        for (p, n) in names.iter().enumerate() {
            let troops: u64 = st
                .cells
                .iter()
                .filter(|c| c.owner == Some(p as PlayerId))
                .map(|c| c.troops as u64)
                .sum();
            println!("  {p} {n:<16} land {:>4} troops {:>7}", counts[p], troops);
        }
    }
}
