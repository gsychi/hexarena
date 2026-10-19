//! Evolution strategies on the policy weights, by self-play against the leaders.
//!
//! Each iteration draws antithetic perturbations `theta +- sigma * eps` of the chosen
//! parameters, plays both sides on the same mirrored 8-player lobbies (3 policy seats,
//! 3 seats of the base bot, 2 roster CPUs) and moves `theta` along the perturbations that
//! placed better. Scores are the eval's: 8-player places, minus the bonus for closing.
//! Every `check` iterations the current weights are written to `--out`.

use crate::{get, Rng};
use hexarena_arena::eval::compare;
use hexarena_sdk::brains::policy::Model;
use hexarena_sdk::{roster, BotParams, Style};
use std::collections::BTreeMap;

fn policy_style() -> Style {
    Style {
        id: "policy".into(),
        name: "policy".into(),
        blurb: String::new(),
        params: BotParams { brain: "policy".into(), ..BotParams::default() },
        program: None,
    }
}

/// Mean gain in places of `model` over `base` on `pairs` mirrored pairs from `seed`.
pub fn score(model: &Model, base: &Style, seed: u64, pairs: usize, tmp: &str) -> Result<f64, String> {
    std::fs::write(tmp, serde_json::to_vec(model).unwrap()).map_err(|e| e.to_string())?;
    std::env::set_var("HEXARENA_POLICY_MODEL", tmp);
    let (r, _) = compare(&policy_style(), base, &roster(), seed, pairs)?;
    Ok(r.gain)
}

/// Indices into `Model::params` that `--subspace` selects: `out` (lin, w2, b2: the
/// output layer and linear skip), `all`, or `bias` (lin entries of the kind one-hots).
fn subspace(m: &Model, which: &str) -> Vec<usize> {
    let (f, h) = (m.f, m.h);
    let lin: Vec<usize> = (0..f).collect();
    let w2: Vec<usize> = (f + h * f + h..f + h * f + 2 * h).collect();
    match which {
        "all" => (0..f + h * f + 2 * h + 1).collect(),
        "bias" => (hexarena_sdk::brains::policy::NG..hexarena_sdk::brains::policy::NG + hexarena_sdk::brains::policy::NK).collect(),
        _ => lin.into_iter().chain(w2).chain([f + h * f + 2 * h]).collect(),
    }
}

pub fn run(f: &BTreeMap<String, String>) -> Result<(), String> {
    let init = f.get("init").ok_or("--init model.json")?;
    let mut model: Model = serde_json::from_slice(&std::fs::read(init).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let out = f.get("out").ok_or("--out")?.clone();
    let against = f.get("against").cloned().unwrap_or_else(|| "pangolin-v9".into());
    let arena = f.get("arena").cloned().unwrap_or_else(|| "/workspace/data/arena".into());
    let base = hexarena_arena::entrants::load_all(std::path::Path::new(&arena))
        .into_iter()
        .find(|e| e.id == against)
        .map(|e| e.style())
        .or_else(|| hexarena_sdk::style(&against))
        .ok_or(format!("unknown base {against}"))?;
    let iters: usize = get(f, "iters", 20);
    let pairs: usize = get(f, "pairs", 2);
    let sigma: f64 = get(f, "sigma", 0.05);
    let lr: f64 = get(f, "lr", 0.02);
    let mut rng = Rng(get(f, "seed", 11));
    let seed0: u64 = get(f, "seed-base", 7_000_000);
    let idx = subspace(&model, f.get("subspace").map_or("out", |s| s.as_str()));
    let tmp = format!("{out}.tmp");
    let mut theta: Vec<f32> = model.params();
    // Scale perturbations to each parameter's size so small and large weights move alike.
    let scale: Vec<f64> = idx.iter().map(|&i| (theta[i].abs() as f64).max(0.05)).collect();
    let started = std::time::Instant::now();
    let mut log = vec![];
    for it in 0..iters {
        let eps: Vec<f64> = idx.iter().map(|_| rng.normal()).collect();
        let seed = seed0 + (it as u64) * 1000;
        let mut plus = theta.clone();
        let mut minus = theta.clone();
        for (k, &i) in idx.iter().enumerate() {
            plus[i] += (sigma * scale[k] * eps[k]) as f32;
            minus[i] -= (sigma * scale[k] * eps[k]) as f32;
        }
        model.set_params(&plus);
        let gp = score(&model, &base, seed, pairs, &tmp)?;
        model.set_params(&minus);
        let gm = score(&model, &base, seed, pairs, &tmp)?;
        let diff = gp - gm;
        for (k, &i) in idx.iter().enumerate() {
            theta[i] += (lr * scale[k] * diff * eps[k]) as f32;
        }
        log.push((gp, gm));
        let mean: f64 = log.iter().map(|(a, b)| (a + b) / 2.0).sum::<f64>() / log.len() as f64;
        eprintln!(
            "iter {it}: gain + {gp:+.2} - {gm:+.2} (running mean {mean:+.3}) {:.0}s",
            started.elapsed().as_secs_f64()
        );
        model.set_params(&theta);
        std::fs::write(&out, serde_json::to_vec(&model).unwrap()).map_err(|e| e.to_string())?;
    }
    Ok(())
}
