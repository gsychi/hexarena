//! Economy layout audit: how each bot arranges Farms, Villages and Quarries, and how many
//! troops per tick its land yields, sampled at fixed ticks.
//!
//! cargo run --release -p hexarena-cli --example layout_yield -- r1.replay.json [r2 ...]
//!
//! Per bot name and sample tick: income per owned hex, income per built hex, building
//! shares, and the histograms "adjacent own Farms per Village" and "adjacent own Villages
//! per Farm". The ideal infinite tiling (Farms on a 1-in-3 sublattice, each ringed by six
//! Villages, each Village touching three Farms) yields 1 + 2/3 + 2 = 3.67 troops/tick/hex.

use std::collections::BTreeMap;

use hexarena_engine::State;
use hexarena_protocol::*;

const SAMPLES: [u32; 4] = [1500, 3000, 6000, 9000];

#[derive(Default)]
struct Acc {
    seats: u64,
    owned: u64,
    income: u64,
    built: u64,
    built_income: u64,
    counts: [u64; 3],
    village_farms: [u64; 7],
    village_quarry: u64,
    farm_villages: [u64; 7],
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let mut acc: BTreeMap<(String, u32), Acc> = BTreeMap::new();
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(replay) = serde_json::from_slice::<Replay>(&bytes) else { continue };
        if replay.rules.hash() != replay.rules_hash {
            continue;
        }
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let Ok(mut st) = State::new(replay.settings(), names.clone()) else { continue };
        let fmt = if names.len() == 2 { "1v1" } else { "ffa" };
        let mut next = 0;
        for tick in &replay.commands {
            st.step(tick);
            if next < SAMPLES.len() && st.tick == SAMPLES[next] {
                sample(&st, &names, fmt, SAMPLES[next], &mut acc);
                next += 1;
            }
        }
    }
    println!(
        "{:<34} {:>5} {:>5} {:>6} {:>6} {:>6} {:>6} {:>13} {:>6}  {:<26} {:<26}",
        "bot [fmt]", "tick", "seats", "land", "inc/hx", "inc/bh", "bh%", "F:V:Q", "V+Q%",
        "farms per village 0..6", "villages per farm 0..6"
    );
    for ((name, tick), a) in &acc {
        if a.seats < 3 {
            continue;
        }
        let pct = |h: &[u64; 7]| {
            let t: u64 = h.iter().sum::<u64>().max(1);
            h.iter().map(|v| format!("{:>3}", v * 100 / t)).collect::<Vec<_>>().join("")
        };
        let v = a.counts[1].max(1) as f64;
        println!(
            "{:<34} {:>5} {:>5} {:>6.0} {:>6.2} {:>6.2} {:>5.0}% {:>4.1}:{:>3.1}:{:>3.1} {:>5.0}%  {:<26} {:<26}",
            name,
            tick,
            a.seats,
            a.owned as f64 / a.seats as f64,
            a.income as f64 / a.owned.max(1) as f64,
            a.built_income as f64 / a.built.max(1) as f64,
            a.built as f64 * 100.0 / a.owned.max(1) as f64,
            a.counts[0] as f64 / v,
            1.0,
            a.counts[2] as f64 / v,
            a.village_quarry as f64 * 100.0 / v,
            pct(&a.village_farms),
            pct(&a.farm_villages),
        );
    }
}

fn sample(st: &State, names: &[String], fmt: &str, tick: u32, acc: &mut BTreeMap<(String, u32), Acc>) {
    for (p, name) in names.iter().enumerate() {
        let p = p as PlayerId;
        if st.eliminated[p as usize] {
            continue;
        }
        let a = acc.entry((format!("{name} [{fmt}]"), tick)).or_default();
        a.seats += 1;
        for h in st.board.hexes() {
            let Some(c) = st.cell(h) else { continue };
            if c.owner != Some(p) {
                continue;
            }
            let inc = st.income(h) as u64;
            a.owned += 1;
            a.income += inc;
            let own = |b: Building| {
                h.neighbors()
                    .iter()
                    .filter(|&&n| st.cell(n).is_some_and(|c| c.owner == Some(p) && c.building == Some(b)))
                    .count()
            };
            match c.building {
                Some(Building::Farm) => {
                    a.counts[0] += 1;
                    a.farm_villages[own(Building::Village)] += 1;
                }
                Some(Building::Village) => {
                    a.counts[1] += 1;
                    a.village_farms[own(Building::Farm)] += 1;
                    a.village_quarry += own(Building::Quarry).min(1) as u64;
                }
                Some(Building::Quarry) => a.counts[2] += 1,
                _ => {}
            }
            if matches!(c.building, Some(Building::Farm | Building::Village | Building::Quarry)) {
                a.built += 1;
                a.built_income += inc;
            }
        }
    }
}
