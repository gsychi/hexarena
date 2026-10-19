//! `league eval` with place histograms per seat type.
//! usage: predator_eval cand.json [pairs] [seed] [league_dir]

use hexarena_league::eval::{gate, Entrant, GateConfig};
use hexarena_league::League;
use hexarena_sdk::BotParams;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let params: BotParams = {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[0]).unwrap()).unwrap();
        serde_json::from_value(v.get("params").cloned().unwrap_or(v)).unwrap()
    };
    let pairs: usize = a.get(1).map_or(16, |s| s.parse().unwrap());
    let seed: u64 = a.get(2).map_or(500_000, |s| s.parse().unwrap());
    let dir = a.get(3).cloned().unwrap_or("/workspace/data/league".into());
    let league = League::load(Path::new(&dir)).unwrap();
    let champ = league.champion_entrant();
    let cand = Entrant { id: "candidate".into(), params };
    let cfg = GateConfig { min_pairs: pairs, max_pairs: pairs, batch_pairs: pairs.min(12), ..GateConfig::default() };
    let (r, logs) = gate(&cand, &champ, &league.pool(), seed, &cfg).unwrap();
    let mut hist = std::collections::BTreeMap::<String, [u32; 8]>::new();
    for l in &logs {
        for (id, p) in l.ids.iter().zip(&l.avp) {
            let key = if *id == "candidate" || *id == champ.id { id.clone() } else { "pool".into() };
            hist.entry(key).or_default()[(p.round() as usize).clamp(1, 8) - 1] += 1;
        }
    }
    println!("vs {} gain {:.3} ± {:.3}  cand {:.3} champ {:.3}  ({} games, {:.0}s)", champ.id, r.gain, r.gain_se, r.challenger_avp, r.champion_avp, r.games, r.secs);
    for (k, h) in hist {
        println!("  {:<10} places 1..8: {:?}", k, h);
    }
}
