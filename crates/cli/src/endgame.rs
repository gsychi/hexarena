//! Endgame lab: replays a live 8-player game to the tick where `--alive` players
//! remain, then plays the rest out from that same state once per candidate for
//! every surviving seat. The other survivors keep their original bots; every bot
//! is warmed up on the last `--warm` ticks of the real game (its moves ignored) so
//! stateful bots enter the endgame with recent history.

use crate::{get, read_replay, seat_style};
use hexarena_arena::eval::avp8;
use hexarena_engine::State;
use hexarena_protocol::*;
use hexarena_sdk::{brains, Bot, Style};
use std::collections::HashMap;
use std::io::Write;

struct Fork {
    /// State `warm` ticks (or fewer) before the fork, for the warm-up.
    early: State,
    at: State,
}

fn fork(replay: &Replay, alive: usize, warm: u32) -> Result<Option<Fork>, String> {
    let names = replay.players.iter().map(|p| p.name.clone()).collect();
    let mut st = State::new(replay.settings(), names)?;
    let mut ring: std::collections::VecDeque<State> = Default::default();
    for cmds in &replay.commands {
        if st.tick % 100 == 0 {
            ring.push_back(st.clone());
            while ring.len() > 1 && st.tick - ring[1].tick >= warm {
                ring.pop_front();
            }
        }
        st.step(cmds);
        if st.eliminated.iter().filter(|e| !**e).count() <= alive && !st.is_over() {
            let early = ring.pop_front().unwrap_or_else(|| st.clone());
            return Ok(Some(Fork { early, at: st }));
        }
    }
    Ok(None)
}

fn survivors(st: &State) -> Vec<PlayerId> {
    (0..st.eliminated.len() as PlayerId).filter(|&p| !st.eliminated[p as usize]).collect()
}

fn holdings(st: &State, p: PlayerId) -> (u64, u64) {
    st.cells
        .iter()
        .filter(|c| c.owner == Some(p))
        .fold((0, 0), |(n, t), c| (n + 1, t + c.troops as u64))
}

/// Plays one sample: `seat` gets `cand`, the other survivors their original bots.
fn sample(replay: &Replay, f: &Fork, styles: &[Option<Style>], seat: PlayerId, cand: &Style) -> Result<(State, f64), String> {
    let alive = survivors(&f.at);
    let mut bots: HashMap<PlayerId, Box<dyn Bot>> = HashMap::new();
    for &p in &alive {
        let s = if p == seat { cand.clone() } else { styles[p as usize].clone().ok_or("missing seat style")? };
        bots.insert(p, brains::build(s)?);
    }
    let mut st = f.early.clone();
    while st.tick < f.at.tick {
        for (&p, bot) in bots.iter_mut() {
            if !st.eliminated[p as usize] {
                bot.act(&st.view(p));
            }
        }
        st.step(&replay.commands[st.tick as usize]);
    }
    debug_assert_eq!(st.state_hash(), f.at.state_hash());
    let mut slowest = 0f64;
    while !st.is_over() {
        let mut cmds: TickCommands = Default::default();
        for &p in &alive {
            if st.eliminated[p as usize] {
                continue;
            }
            let view = st.view(p);
            let t0 = std::time::Instant::now();
            let c = bots.get_mut(&p).unwrap().act(&view);
            if p == seat {
                slowest = slowest.max(t0.elapsed().as_secs_f64() * 1000.0);
            }
            cmds.push((p, c));
        }
        st.step(&cmds);
    }
    Ok((st, slowest))
}

/// `hexarena endgame --replays a.json,b.json --cands race.json,blitz.json --out lab.jsonl
/// [--alive 3] [--warm 600] [--reps 1] [--skip-seat-bot prefix]`
pub fn main(f: &HashMap<String, String>) -> Result<(), String> {
    let replays: Vec<&str> = f.get("replays").ok_or("--replays a.json,b.json")?.split(',').collect();
    let cands: Vec<Style> = f
        .get("cands")
        .ok_or("--cands a.json,b.json")?
        .split(',')
        .map(|c| seat_style(c.trim(), f))
        .collect::<Result<_, _>>()?;
    let alive_n: usize = get(f, "alive", 3);
    let warm: u32 = get(f, "warm", 600);
    let reps: u32 = get(f, "reps", 1);
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(f.get("out").ok_or("--out lab.jsonl")?)
        .map_err(|e| e.to_string())?;
    for path in replays {
        let replay = read_replay(path)?;
        let seats_path = path.replace(".json", ".seats.json");
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(&seats_path).map_err(|e| format!("{seats_path}: {e}"))?)
            .map_err(|e| e.to_string())?;
        let bots: Vec<String> = meta["bots"].as_array().ok_or("seats.json: bots")?.iter().map(|b| b.as_str().unwrap_or("").to_string()).collect();
        let Some(fk) = fork(&replay, alive_n, warm)? else {
            eprintln!("{path}: never reaches {alive_n} alive");
            continue;
        };
        let alive = survivors(&fk.at);
        let styles: Vec<Option<Style>> = bots
            .iter()
            .enumerate()
            .map(|(p, b)| if alive.contains(&(p as PlayerId)) { seat_style(b, f).ok() } else { None })
            .collect();
        if alive.iter().any(|&p| styles[p as usize].is_none()) {
            eprintln!("{path}: a survivor's bot is not loadable ({:?})", alive.iter().map(|&p| &bots[p as usize]).collect::<Vec<_>>());
            continue;
        }
        let passable = fk.at.terrain.iter().flatten().filter(|t| t.passable()).count();
        let hold: Vec<(u64, u64)> = alive.iter().map(|&p| holdings(&fk.at, p)).collect();
        let orig = Some(&replay.result);
        for &seat in &alive {
            if let Some(skip) = f.get("skip-seat-bot") {
                if bots[seat as usize].starts_with(skip.as_str()) {
                    continue;
                }
            }
            for cand in &cands {
                for rep in 0..reps {
                    let (st, slowest) = sample(&replay, &fk, &styles, seat, cand)?;
                    let r = st.result.clone().ok_or("unfinished")?;
                    let pl = r.placement(seat);
                    let orig_pl = orig.map(|r| r.placement(seat)).map(|p| avp8(p.place_start, p.place_end, 8));
                    let rec = serde_json::json!({
                        "replay": path, "fork_tick": fk.at.tick, "seat": seat, "seat_bot": bots[seat as usize],
                        "alive": alive, "alive_bots": alive.iter().map(|&p| &bots[p as usize]).collect::<Vec<_>>(),
                        "land": hold.iter().map(|h| h.0).collect::<Vec<_>>(), "troops": hold.iter().map(|h| h.1).collect::<Vec<_>>(),
                        "passable": passable, "cand": cand.id, "rep": rep,
                        "place": avp8(pl.place_start, pl.place_end, 8), "orig_place": orig_pl,
                        "final_tick": r.final_tick, "conquest": r.final_tick < st.rules().max_ticks,
                        "winner": r.winner(), "slowest_ms": slowest,
                    });
                    writeln!(out, "{rec}").map_err(|e| e.to_string())?;
                    out.flush().ok();
                    eprintln!("{path} t{} seat {seat} ({}) {} -> {:.2} (tick {})", fk.at.tick, bots[seat as usize], cand.id, avp8(pl.place_start, pl.place_end, 8), r.final_tick);
                }
            }
        }
    }
    Ok(())
}
