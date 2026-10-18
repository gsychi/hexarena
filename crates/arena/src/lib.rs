//! Runs CPU matches on the engine and converts replays into the viewer format.
//! Shared by the CLI and the server so both produce identical artifacts.

use hexarena_engine::{build_replay, State};
use hexarena_protocol::*;
use hexarena_sdk::{Bot, Expander, Style};
use std::collections::BTreeMap;

/// Derives the three independent seeds from one match seed.
pub fn settings(players: u16, seed: u64) -> MatchSettings {
    MatchSettings {
        players,
        terrain_seed: seed,
        spawn_seed: seed.wrapping_mul(31).wrapping_add(7),
        match_seed: seed.wrapping_mul(13).wrapping_add(5),
        rules: RulesConfig::default(),
    }
}

/// "Raider", "Raider 2", ... so duplicate styles stay distinguishable.
pub fn seat_names(styles: &[Style]) -> Vec<String> {
    let mut seen: BTreeMap<&str, u32> = BTreeMap::new();
    styles
        .iter()
        .map(|s| {
            let n = seen.entry(s.name.as_str()).or_insert(0);
            *n += 1;
            if *n == 1 {
                s.name.to_string()
            } else {
                format!("{} {}", s.name, n)
            }
        })
        .collect()
}

pub struct Played {
    pub state: State,
    pub commands: Vec<TickCommands>,
    pub events: Vec<Vec<Event>>,
}

impl Played {
    pub fn result(&self) -> MatchResult {
        self.state
            .result
            .clone()
            .unwrap_or_else(|| self.state.final_result())
    }

    pub fn replay(&self) -> Replay {
        build_replay(&self.state, self.commands.clone(), self.events.clone())
    }
}

/// Plays one match with one CPU per seat, in seat order, recording a full replay.
pub fn play(seed: u64, styles: &[Style], max_ticks: u32) -> Result<Played, String> {
    run(seed, styles, max_ticks, true)
}

/// Plays one match without recording commands or events: the fast path for evaluation.
pub fn play_result(seed: u64, styles: &[Style], max_ticks: u32) -> Result<MatchResult, String> {
    run(seed, styles, max_ticks, false).map(|p| p.result())
}

fn run(seed: u64, styles: &[Style], max_ticks: u32, record: bool) -> Result<Played, String> {
    let s = settings(styles.len() as u16, seed);
    let mut st = State::new(s, seat_names(styles))?;
    let mut bots: Vec<Box<dyn Bot>> = styles
        .iter()
        .map(|s| Box::new(Expander::new(s.clone())) as Box<dyn Bot>)
        .collect();
    let (mut commands, mut events) = (vec![], vec![]);
    while !st.is_over() && st.tick < max_ticks {
        let cmds: TickCommands = (0..styles.len() as PlayerId)
            .filter(|&p| !st.eliminated[p as usize])
            .map(|p| (p, bots[p as usize].act(&st.view(p))))
            .collect();
        st.step(&cmds);
        if record {
            commands.push(cmds);
            events.push(st.last_events.clone());
        }
    }
    Ok(Played {
        state: st,
        commands,
        events,
    })
}

/// Viewer document for a replay. Frame record stride is 9: idx, owner, troops,
/// building, production remaining, link a, link b, link c (hex index or -1),
/// link cooldown. Frames after the first use the `delta-v2` encoding.
pub fn viewer_json(replay: &Replay) -> Result<serde_json::Value, String> {
    let mut st = State::new(
        replay.settings(),
        replay.players.iter().map(|p| p.name.clone()).collect(),
    )?;
    let initial: Vec<u32> = st
        .cells
        .iter()
        .map(|c| if c.owner.is_none() { c.troops } else { 0 })
        .collect();
    let mut prev = frame(&st, &initial);
    let mut frames = vec![prev.clone()];
    let mut vel = Velocity::new();
    let mut events = vec![serde_json::json!([])];
    for cmds in &replay.commands {
        st.step(cmds);
        let next = frame(&st, &initial);
        frames.push(frame_delta(&prev, &next, &mut vel));
        prev = next;
        events.push(serde_json::to_value(&st.last_events).unwrap());
    }
    Ok(serde_json::json!({
        "radius": st.board.radius, "fog": st.rules().fog_radius, "names": st.names, "rules": st.rules(),
        "terrain": st.terrain,
        "neutral": initial,
        "frame_encoding": "delta-v2", "frames": frames, "events": events,
        "result": replay.result, "building_report": st.building_report(),
    }))
}

fn frame(st: &State, initial: &[u32]) -> Vec<i32> {
    let idx = |h: Option<Hex>| h.map(|l| st.board.index(l).unwrap() as i32).unwrap_or(-1);
    let mut v = vec![];
    for h in st.board.hexes() {
        let c = st.cell(h).unwrap();
        let i = st.board.index(h).unwrap();
        if c.owner.is_none() && c.building.is_none() && c.troops == initial[i] {
            continue;
        }
        let (remaining, links, cooldown) = visible_building(st, h);
        v.extend([
            i as i32,
            c.owner.map(|o| o as i32).unwrap_or(-1),
            c.troops as i32,
            c.building.map(|b| b as i32 + 1).unwrap_or(0),
            remaining as i32,
            idx(links[0]),
            idx(links[1]),
            idx(links[2]),
            cooldown as i32,
        ]);
    }
    v
}

/// (production remaining, links, cooldown) for a hex, using the same logic the View exposes.
fn visible_building(st: &State, h: Hex) -> (u32, [Option<Hex>; 3], u32) {
    let c = st.cell(h).unwrap();
    let remaining = match c.building {
        Some(Building::Farm | Building::Quarry) => {
            st.rules().extractor_period.saturating_sub(c.progress)
        }
        _ => 0,
    };
    let links = match c.building {
        Some(Building::Village) => [st.supporting_farm(h), None, None],
        _ => c.links,
    };
    (remaining, links, c.link_ready_tick.saturating_sub(st.tick))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hexarena_engine::replay_state;
    use hexarena_sdk::roster;

    #[test]
    fn same_seed_and_lineup_replay_identically() {
        let lineup = roster();
        let a = play(42, &lineup, 300).unwrap().replay();
        let b = play(42, &lineup, 300).unwrap().replay();
        assert_eq!(a.checksum, b.checksum);
        let st = replay_state(&a).unwrap();
        assert_eq!(format!("{:016x}", st.state_hash()), a.checksum);
    }

    #[test]
    fn duplicate_styles_get_numbered_names() {
        let r = roster();
        let names = seat_names(&[r[2].clone(), r[0].clone(), r[2].clone()]);
        assert_eq!(names, ["Raider", "Expander", "Raider 2"]);
    }

    #[test]
    fn viewer_has_one_frame_per_tick() {
        let replay = play(3, &roster()[..2], 120).unwrap().replay();
        let doc = viewer_json(&replay).unwrap();
        assert_eq!(
            doc["frames"].as_array().unwrap().len(),
            replay.commands.len() + 1
        );
    }
}

/// Bit set in a delta mask when a cell left the frame.
const DELTA_REMOVED: i32 = 1 << 8;

/// Record fields (after idx) that are predicted to keep last tick's rate of
/// change: troops, production remaining, link cooldown.
const DELTA_PREDICTED: [usize; 3] = [1, 3, 7];

type Record = [i32; 8];
type Velocity = BTreeMap<i32, [i32; 3]>;

fn records(f: &[i32]) -> BTreeMap<i32, Record> {
    f.chunks(9)
        .map(|c| (c[0], c[1..].try_into().unwrap()))
        .collect()
}

/// Frames after the first are deltas: groups of `idx, mask, values...` where
/// bit k of `mask` means field k (0 = owner) differs from its prediction and
/// the true value follows, in field order. Predicted fields assume the same
/// change as the previous tick; others assume no change.
fn frame_delta(prev: &[i32], next: &[i32], vel: &mut Velocity) -> Vec<i32> {
    let (a, b) = (records(prev), records(next));
    let mut out = vec![];
    let mut next_vel = Velocity::new();
    for (idx, new) in &b {
        let old = a.get(idx);
        let v = vel.get(idx).copied().unwrap_or([0; 3]);
        let mut mask = 0;
        let mut vals = vec![];
        for k in 0..8 {
            let expected = old.map(|o| match DELTA_PREDICTED.iter().position(|&p| p == k) {
                Some(j) => o[k] + v[j],
                None => o[k],
            });
            if expected != Some(new[k]) {
                mask |= 1 << k;
                vals.push(new[k]);
            }
        }
        next_vel.insert(
            *idx,
            old.map_or([0; 3], |o| DELTA_PREDICTED.map(|k| new[k] - o[k])),
        );
        if mask != 0 {
            out.extend([*idx, mask]);
            out.extend(vals);
        }
    }
    for idx in a.keys().filter(|i| !b.contains_key(i)) {
        out.extend([*idx, DELTA_REMOVED]);
    }
    *vel = next_vel;
    out
}
