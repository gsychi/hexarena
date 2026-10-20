//! How a bot's seats die in 8-player games: when, to whom, and from what position.
//!
//! cargo run --release -p hexarena-cli --example death_stats -- PREFIX r1.json [r2.json ...]
//! Only seats whose name starts with PREFIX are studied. For each death it reports the
//! killer (the rival that took most of our hexes in the last 500 ticks), our position
//! 1000 and 500 ticks earlier, and how the last land went; survivors at tick 7000 are the
//! comparison.

use hexarena_engine::State;
use hexarena_protocol::*;
use std::collections::BTreeMap;

const SNAP: u32 = 100;

#[derive(Default, Clone, Copy)]
struct Pos {
    land: u64,
    share_milli: u64,
    rank: u64,
    rivals_touching: u64,
    stronger_touching: u64,
    troops: u64,
    /// % of our troops on hexes touching the killer.
    on_killer_pct: u64,
    /// % of our troops on hexes touching some rival.
    on_front_pct: u64,
    /// Our troops per killer troop, in %.
    vs_killer_pct: u64,
    killer_land: u64,
}

fn position(st: &State, me: usize, killer: Option<usize>) -> Pos {
    let n = st.eliminated.len();
    let mut land = vec![0u64; n];
    let mut troops = vec![0u64; n];
    let mut touching = vec![false; n];
    let (mut on_killer, mut on_front) = (0u64, 0u64);
    for h in st.board.hexes() {
        let i = st.board.index(h).unwrap();
        let c = &st.cells[i];
        let Some(o) = c.owner else { continue };
        land[o as usize] += 1;
        troops[o as usize] += c.troops as u64;
        if o as usize != me {
            continue;
        }
        let (mut k, mut f) = (false, false);
        for nb in h.neighbors() {
            let Some(j) = st.board.index(nb) else { continue };
            if let Some(r) = st.cells[j].owner.filter(|&r| r as usize != me) {
                touching[r as usize] = true;
                f = true;
                k |= Some(r as usize) == killer;
            }
        }
        if k {
            on_killer += c.troops as u64;
        }
        if f {
            on_front += c.troops as u64;
        }
    }
    let total: u64 = land.iter().sum();
    let mine = land[me];
    Pos {
        land: mine,
        share_milli: mine * 1000 / total.max(1),
        rank: 1 + land.iter().filter(|&&l| l > mine).count() as u64,
        rivals_touching: touching.iter().filter(|&&t| t).count() as u64,
        stronger_touching: (0..n).filter(|&r| touching[r] && land[r] > mine).count() as u64,
        troops: troops[me],
        on_killer_pct: on_killer * 100 / troops[me].max(1),
        on_front_pct: on_front * 100 / troops[me].max(1),
        vs_killer_pct: killer.map_or(0, |k| troops[me] * 100 / troops[k].max(1)),
        killer_land: killer.map_or(0, |k| land[k]),
    }
}

#[derive(Default)]
struct Sum {
    n: u64,
    v: [u64; 10],
}

impl Sum {
    fn add(&mut self, p: Pos) {
        self.n += 1;
        let f = [
            p.land,
            p.share_milli,
            p.rank,
            p.rivals_touching,
            p.stronger_touching,
            p.troops,
            p.on_killer_pct,
            p.on_front_pct,
            p.vs_killer_pct,
            p.killer_land,
        ];
        for (a, b) in self.v.iter_mut().zip(f) {
            *a += b;
        }
    }
    fn show(&self, label: &str) {
        let m = |i: usize| self.v[i] as f64 / self.n.max(1) as f64;
        println!(
            "{label:<22} n {:>4} | land {:>6.1} share {:>5.1}% rank {:.2} rivals touching {:.2} (stronger {:.2}) troops {:>6.0} | on killer front {:>4.1}% on any front {:>4.1}% troops vs killer {:>5.0}% killer land {:>5.1}",
            self.n,
            m(0),
            m(1) / 10.0,
            m(2),
            m(3),
            m(4),
            m(5),
            m(6),
            m(7),
            m(8),
            m(9)
        );
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let prefix = args.next().unwrap();
    let paths: Vec<String> = args.collect();
    let (mut seats, mut deaths) = (0u64, 0u64);
    let mut death_bin = [0u64; 10];
    let mut mid_bin = [0u64; 12];
    let mut alive_at_death: BTreeMap<usize, u64> = BTreeMap::new();
    let mut killer_rank: BTreeMap<u64, u64> = BTreeMap::new();
    let mut killers: BTreeMap<String, u64> = BTreeMap::new();
    let (mut lost_cap, mut lost_seal, mut lost_killer) = (0u64, 0u64, 0u64);
    let (mut at1000, mut at500, mut survivors) = (Sum::default(), Sum::default(), Sum::default());
    let mut peak_drop = (0u64, 0u64);
    for path in &paths {
        let replay: Replay = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let names: Vec<String> = replay.players.iter().map(|p| p.name.clone()).collect();
        let n = names.len();
        if n != 8 {
            continue;
        }
        let mine: Vec<bool> = names.iter().map(|x| x.starts_with(&prefix)).collect();
        let mut st = State::new(replay.settings(), names.clone()).unwrap();
        let mut snaps: Vec<State> = vec![];
        // (tick, victim, taker, hexes, by seal)
        let mut losses: Vec<(u32, usize, usize, u64, bool)> = vec![];
        let mut died: Vec<Option<(u32, usize)>> = vec![None; n];
        let mut peak = vec![0u64; n];
        for cmds in &replay.commands {
            if st.tick % SNAP == 0 {
                snaps.push(st.clone());
            }
            st.step(cmds);
            let t = st.tick;
            for e in &st.last_events {
                match e {
                    Event::Captured { player, previous_owner: Some(o), .. } if mine[*o as usize] => {
                        losses.push((t, *o as usize, *player as usize, 1, false));
                    }
                    Event::Surrendered { capturer, from, hexes } if mine[*from as usize] => {
                        losses.push((t, *from as usize, *capturer as usize, hexes.len() as u64, true));
                    }
                    _ => {}
                }
            }
            if t % SNAP == 0 {
                for p in (0..n).filter(|&p| mine[p]) {
                    let l = st.cells.iter().filter(|c| c.owner == Some(p as PlayerId)).count() as u64;
                    peak[p] = peak[p].max(l);
                }
            }
            let alive = st.eliminated.iter().filter(|&&e| !e).count() + 1;
            for p in 0..n {
                if mine[p] && died[p].is_none() && st.eliminated[p] {
                    died[p] = Some((t, alive));
                }
            }
        }
        for p in (0..n).filter(|&p| mine[p]) {
            seats += 1;
            let Some((t, alive)) = died[p] else {
                if let Some(s) = snaps.get(70) {
                    if !s.eliminated[p] {
                        survivors.add(position(s, p, None));
                    }
                }
                continue;
            };
            deaths += 1;
            death_bin[(t as usize / 1000).min(9)] += 1;
            if (3000..6000).contains(&t) {
                mid_bin[(t as usize - 3000) / 250] += 1;
            }
            *alive_at_death.entry(alive).or_default() += 1;
            let mut by = vec![0u64; n];
            for &(lt, v, k, c, seal) in &losses {
                if v == p && lt + 1000 > t {
                    if seal {
                        lost_seal += c;
                    } else {
                        lost_cap += c;
                    }
                }
                if v == p && lt + 500 > t {
                    by[k] += c;
                }
            }
            let killer = (0..n).max_by_key(|&k| by[k]).unwrap();
            for &(lt, v, k, c, _) in &losses {
                if v == p && lt + 1000 > t && k == killer {
                    lost_killer += c;
                }
            }
            let kname = names[killer].split(' ').next().unwrap_or("").to_string();
            *killers.entry(kname).or_default() += 1;
            let snap = |back: u32| &snaps[(t.saturating_sub(back) / SNAP) as usize];
            let p1 = position(snap(1000), p, Some(killer));
            at1000.add(p1);
            at500.add(position(snap(500), p, Some(killer)));
            let kp = position(snap(1000), killer, None);
            *killer_rank.entry(kp.rank).or_default() += 1;
            peak_drop.0 += peak[p];
            peak_drop.1 += 1;
        }
    }
    println!("{} games' seats {seats}, deaths {deaths} ({:.0}%)", paths.len(), deaths as f64 * 100.0 / seats.max(1) as f64);
    println!("deaths by 1000-tick bin: {death_bin:?}");
    println!("deaths from 3000 to 6000 by 250 ticks: {mid_bin:?}");
    println!("players alive when we died (us included): {alive_at_death:?}");
    println!("killer's land rank 1000 ticks before: {killer_rank:?}");
    println!("killers: {killers:?}");
    let lost = lost_cap + lost_seal;
    println!(
        "last 1000 ticks of land lost: {lost} hexes, {:.0}% to captures, {:.0}% to seals, {:.0}% to the killer; mean peak land {:.1}",
        lost_cap as f64 * 100.0 / lost.max(1) as f64,
        lost_seal as f64 * 100.0 / lost.max(1) as f64,
        lost_killer as f64 * 100.0 / lost.max(1) as f64,
        peak_drop.0 as f64 / peak_drop.1.max(1) as f64
    );
    at1000.show("1000 ticks before death");
    at500.show("500 ticks before death");
    survivors.show("survivors at 7000");
}
