//! Genome mutation and crossover for challengers.

use crate::rng::Rng;
use hexarena_protocol::Building;
use hexarena_sdk::BotParams;

/// A numeric gene: name, legal range, and accessors.
struct Gene {
    name: &'static str,
    min: i64,
    max: i64,
    get: fn(&BotParams) -> i64,
    set: fn(&mut BotParams, i64),
}

const GENES: &[Gene] = &[
    Gene { name: "attack_margin_pct", min: 100, max: 260, get: |p| p.attack_margin_pct as i64, set: |p, v| p.attack_margin_pct = v as u64 },
    Gene { name: "enemy_bonus", min: -40, max: 160, get: |p| p.enemy_bonus, set: |p, v| p.enemy_bonus = v },
    Gene { name: "build_cap", min: 0, max: 30, get: |p| p.build_cap as i64, set: |p, v| p.build_cap = v as usize },
    Gene { name: "build_every", min: 1, max: 60, get: |p| p.build_every as i64, set: |p, v| p.build_every = v as u32 },
    Gene { name: "gather_every", min: 1, max: 24, get: |p| p.gather_every as i64, set: |p, v| p.gather_every = v as u32 },
    Gene { name: "land_value", min: 0, max: 160, get: |p| p.land_value, set: |p, v| p.land_value = v },
    Gene { name: "hug_value", min: 0, max: 40, get: |p| p.hug_value, set: |p, v| p.hug_value = v },
    Gene { name: "focus_bonus", min: 0, max: 160, get: |p| p.focus_bonus, set: |p, v| p.focus_bonus = v },
    Gene { name: "kill_value_pct", min: 0, max: 200, get: |p| p.kill_value_pct, set: |p, v| p.kill_value_pct = v },
    Gene { name: "grab_every", min: 0, max: 10, get: |p| p.grab_every as i64, set: |p, v| p.grab_every = v as u32 },
    Gene { name: "settle_every", min: 0, max: 6, get: |p| p.settle_every as i64, set: |p, v| p.settle_every = v as u32 },
    Gene { name: "endgame_ticks", min: 0, max: 3000, get: |p| p.endgame_ticks as i64, set: |p, v| p.endgame_ticks = v as u32 },
    Gene { name: "endgame_margin_pct", min: 100, max: 400, get: |p| p.endgame_margin_pct as i64, set: |p, v| p.endgame_margin_pct = v as u64 },
    Gene { name: "endgame_enemy_bonus", min: -40, max: 160, get: |p| p.endgame_enemy_bonus, set: |p, v| p.endgame_enemy_bonus = v },
];

const KINDS: [Building; 4] = [Building::Farm, Building::Village, Building::Quarry, Building::Wall];

fn kind_name(b: Building) -> &'static str {
    match b {
        Building::Farm => "Farm",
        Building::Village => "Village",
        Building::Quarry => "Quarry",
        Building::Wall => "Wall",
    }
}

fn kinds_label(v: &[Building]) -> String {
    if v.is_empty() {
        "none".into()
    } else {
        v.iter().map(|k| kind_name(*k)).collect::<Vec<_>>().join("/")
    }
}

/// Human-readable gene differences, e.g. `land_value 40→52`.
pub fn diff(from: &BotParams, to: &BotParams) -> Vec<String> {
    let mut out: Vec<String> = GENES
        .iter()
        .filter(|g| (g.get)(from) != (g.get)(to))
        .map(|g| format!("{} {}→{}", g.name, (g.get)(from), (g.get)(to)))
        .collect();
    if from.march != to.march {
        out.push(format!("march {}→{}", from.march, to.march));
    }
    if from.kinds != to.kinds {
        out.push(format!("kinds {}→{}", kinds_label(&from.kinds), kinds_label(&to.kinds)));
    }
    out
}

/// Changes 1–3 genes of `p`; never returns `p` unchanged.
pub fn mutate(p: &BotParams, rng: &mut Rng) -> BotParams {
    loop {
        let out = mutate_once(p, rng);
        if out != *p {
            return out;
        }
    }
}

fn mutate_once(p: &BotParams, rng: &mut Rng) -> BotParams {
    let mut out = p.clone();
    let changes = 1 + rng.below(3);
    for _ in 0..changes {
        let slot = rng.below(GENES.len() + 2);
        if slot < GENES.len() {
            let g = &GENES[slot];
            let step = ((g.max - g.min) / 8).max(1);
            let delta = 1 + rng.below(step as usize) as i64;
            let delta = if rng.chance(0.5) { delta } else { -delta };
            let value = ((g.get)(&out) + delta).clamp(g.min, g.max);
            (g.set)(&mut out, value);
        } else if slot == GENES.len() {
            if rng.chance(0.25) {
                out.march = !out.march;
            }
        } else {
            mutate_kinds(&mut out.kinds, rng);
        }
    }
    out
}

fn mutate_kinds(kinds: &mut Vec<Building>, rng: &mut Rng) {
    match rng.below(3) {
        0 if kinds.len() < 6 => {
            let at = rng.below(kinds.len() + 1);
            kinds.insert(at, KINDS[rng.below(KINDS.len())]);
        }
        1 if kinds.len() > 1 => {
            kinds.remove(rng.below(kinds.len()));
        }
        _ if !kinds.is_empty() => {
            let i = rng.below(kinds.len());
            kinds[i] = KINDS[rng.below(KINDS.len())];
        }
        _ => kinds.push(KINDS[rng.below(KINDS.len())]),
    }
}

/// Each gene taken from `a` or `b` with equal odds.
pub fn crossover(a: &BotParams, b: &BotParams, rng: &mut Rng) -> BotParams {
    let mut out = a.clone();
    for g in GENES {
        if rng.chance(0.5) {
            (g.set)(&mut out, (g.get)(b));
        }
    }
    if rng.chance(0.5) {
        out.march = b.march;
    }
    if rng.chance(0.5) {
        out.kinds = b.kinds.clone();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_changes_something_and_stays_in_range() {
        let base = BotParams::default();
        let mut rng = Rng::new(9);
        let mut changed = 0;
        for _ in 0..200 {
            let m = mutate(&base, &mut rng);
            if m != base {
                changed += 1;
            }
            for g in GENES {
                assert!((g.min..=g.max).contains(&(g.get)(&m)), "{}", g.name);
            }
            assert!(m.kinds.len() <= 6);
        }
        assert_eq!(changed, 200);
    }

    #[test]
    fn diff_lists_changed_genes() {
        let a = BotParams::default();
        let b = BotParams { land_value: 52, march: false, ..a.clone() };
        assert_eq!(diff(&a, &b), ["land_value 40→52", "march true→false"]);
    }
}
