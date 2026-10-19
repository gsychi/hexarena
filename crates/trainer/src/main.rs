//! Training tools for the `policy` brain.
//!
//! `extract` replays finished v0.6 games from `data/matches`, rebuilds the exact view a
//! chosen seat saw on sampled ticks, lists the policy's candidates and features for it,
//! and records which candidate that seat actually played. Only a stratified sample of
//! the other candidates is kept, with log-weights that undo the sampling in a softmax.
//! `bench` times the policy's moves over full games. `es` tunes the weights by self-play.

mod es;

use hexarena_engine::State;
use hexarena_protocol::*;
use hexarena_sdk::brains::policy::{Featurizer, History, Kind, F, NK};
use std::collections::BTreeMap;
use std::io::Write;

fn flags(args: &[String]) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(k) = args[i].strip_prefix("--") {
            match args.get(i + 1) {
                Some(v) if !v.starts_with("--") => {
                    m.insert(k.to_string(), v.clone());
                    i += 1;
                }
                _ => {
                    m.insert(k.to_string(), String::new());
                }
            }
        }
        i += 1;
    }
    m
}

pub fn get<T: std::str::FromStr>(f: &BTreeMap<String, String>, k: &str, d: T) -> T {
    f.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// SplitMix64.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn normal(&mut self) -> f64 {
        let u = self.unit().max(1e-12);
        let v = self.unit();
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (1 - exp + 13);
        return sign | m as u16;
    }
    if exp >= 31 {
        return sign | 0x7bff;
    }
    let mut out = sign as u32 | ((exp as u32) << 10) | (man >> 13);
    if man & 0x1000 != 0 {
        out += 1;
    }
    out as u16
}

/// One teacher seat: (match id, seat, bot id, place, seats).
type SeatRef = (String, u16, String, u16, u16);

struct Out {
    x: std::io::BufWriter<std::fs::File>,
    rows: std::io::BufWriter<std::fs::File>,
    groups: std::io::BufWriter<std::fs::File>,
    nrows: u32,
}

impl Out {
    fn row(&mut self, feats: &[f32], corr: f32, kind: Kind) {
        let mut buf = Vec::with_capacity(F * 2);
        for &v in feats {
            buf.extend_from_slice(&f16_bits(v).to_le_bytes());
        }
        self.x.write_all(&buf).unwrap();
        self.rows.write_all(&corr.to_le_bytes()).unwrap();
        self.rows.write_all(&(kind.index() as f32).to_le_bytes()).unwrap();
        self.nrows += 1;
    }
}

/// Keeps the chosen candidate, a sample of its siblings (same hex) and a sample of every
/// kind, each with `ln(population / kept)` so a softmax over the kept rows estimates the
/// full one.
#[allow(clippy::too_many_arguments)]
fn write_sample(out: &mut Out, fz: &Featurizer, chosen: usize, k_kind: usize, k_sib: usize, hard: Option<(&[f32], usize)>, rng: &mut Rng, meta: [u32; 3]) {
    let start = out.nrows;
    let cands = &fz.cands;
    let a = cands[chosen].a;
    let mut strata: Vec<Vec<usize>> = vec![vec![]; NK + 1];
    let mut top: Vec<usize> = vec![];
    if let Some((scores, k)) = hard {
        let mut order: Vec<usize> = (0..cands.len()).filter(|&i| i != chosen).collect();
        order.sort_by(|&x, &y| scores[y].partial_cmp(&scores[x]).unwrap());
        top = order.into_iter().take(k).collect();
    }
    out.row(&fz.feats[chosen * F..(chosen + 1) * F], 0.0, cands[chosen].kind);
    for &i in &top {
        out.row(&fz.feats[i * F..(i + 1) * F], 0.0, cands[i].kind);
    }
    for (i, c) in cands.iter().enumerate() {
        if i == chosen || top.contains(&i) {
            continue;
        }
        let s = if c.a == a && c.kind != Kind::NoOp { NK } else { c.kind.index() };
        strata[s].push(i);
    }
    for (s, members) in strata.iter_mut().enumerate() {
        let k = if s == NK { k_sib } else { k_kind };
        let n = members.len();
        if n == 0 {
            continue;
        }
        let take = k.min(n);
        for j in 0..take {
            let r = j + rng.below(n - j);
            members.swap(j, r);
        }
        let corr = (n as f32 / take as f32).ln();
        for &i in &members[..take] {
            out.row(&fz.feats[i * F..(i + 1) * F], corr, cands[i].kind);
        }
    }
    let n = out.nrows - start;
    for v in [start, n, meta[0], meta[1], meta[2]] {
        out.groups.write_all(&v.to_le_bytes()).unwrap();
    }
}

fn extract(f: &BTreeMap<String, String>) -> Result<(), String> {
    let list: Vec<SeatRef> = serde_json::from_slice(&std::fs::read(f.get("list").ok_or("--list")?).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let dir = f.get("matches").cloned().unwrap_or_else(|| "/workspace/data/matches".into());
    let prefix = f.get("out").ok_or("--out")?.clone();
    let every: u64 = get(f, "every", 40);
    let games: usize = get(f, "games", 50);
    let k_kind: usize = get(f, "k", 4);
    let k_sib: usize = get(f, "sib", 8);
    let teachers: Vec<String> = f.get("teachers").map(|s| s.split(',').map(String::from).collect()).unwrap_or_default();
    let mut rng = Rng(get(f, "seed", 7));
    let k_hard: usize = get(f, "hard", 16);
    let scorer = match f.get("model") {
        Some(p) => {
            let m: hexarena_sdk::brains::policy::Model =
                serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Some(hexarena_sdk::brains::policy::Folded::new(&m))
        }
        None => None,
    };
    let mut scores: Vec<f32> = vec![];
    let mut by_match: BTreeMap<String, Vec<SeatRef>> = BTreeMap::new();
    for s in list {
        if teachers.is_empty() || teachers.contains(&s.2) {
            by_match.entry(s.0.clone()).or_default().push(s);
        }
    }
    // Spread the chosen games over the whole record, not just the oldest.
    let ids: Vec<String> = by_match.keys().cloned().collect();
    let stride = (ids.len() / games.max(1)).max(1);
    let open = |ext: &str| std::io::BufWriter::new(std::fs::File::create(format!("{prefix}.{ext}")).unwrap());
    let mut out = Out { x: open("x.f16"), rows: open("rows.f32"), groups: open("groups.u32"), nrows: 0 };
    let mut tally: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut kinds = [0u64; NK];
    let mut unmatched_kinds: BTreeMap<String, u64> = BTreeMap::new();
    let mut ncands = 0u64;
    let started = std::time::Instant::now();
    for (gi, id) in ids.iter().step_by(stride).take(games).enumerate() {
        let seats = &by_match[id];
        let path = format!("{dir}/{id}.replay.json");
        let replay: Replay = match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{id}: {e}");
                continue;
            }
        };
        let names = replay.players.iter().map(|p| p.name.clone()).collect();
        let mut st = State::new(replay.settings(), names)?;
        let mut fz: Vec<Featurizer> = seats.iter().map(|_| Featurizer::new()).collect();
        let mut hist: Vec<History> = seats.iter().map(|_| History::default()).collect();
        for (tick, cmds) in replay.commands.iter().enumerate() {
            for (k, s) in seats.iter().enumerate() {
                let seat = s.1;
                if st.eliminated[seat as usize] {
                    continue;
                }
                let action = cmds.iter().find(|(p, _)| *p == seat).map(|(_, c)| c.action.clone()).unwrap_or(Action::NoOp);
                let captured = match &action {
                    Action::Move { to, .. } => st.cell(*to).is_some_and(|c| c.owner != Some(seat)),
                    _ => false,
                };
                let sample = tick == 0 || rng.below(every as usize) == 0;
                if sample {
                    let view = st.view(seat);
                    fz[k].build(&view, &hist[k]);
                    ncands += fz[k].cands.len() as u64;
                    match fz[k].find(&action) {
                        Some(i) => {
                            *tally.entry("matched").or_default() += 1;
                            kinds[fz[k].cands[i].kind.index()] += 1;
                            if tick > 0 {
                                let hard = scorer.as_ref().map(|m| {
                                    m.score_all(&fz[k].feats, &mut scores);
                                    (&scores[..], k_hard)
                                });
                                write_sample(&mut out, &fz[k], i, k_kind, k_sib, hard, &mut rng, [tick as u32, s.3 as u32, gi as u32]);
                            }
                        }
                        None => {
                            *tally.entry("unmatched").or_default() += 1;
                            let what = match &action {
                                Action::Move { to, .. } => {
                                    if st.cell(*to).is_some_and(|c| c.owner == Some(seat)) { "relay" } else { "capture" }
                                }
                                Action::Build { building, .. } => match building {
                                    Building::Farm => "farm",
                                    Building::Village => "village",
                                    Building::Quarry => "quarry",
                                    Building::Wall => "wall",
                                },
                                Action::Gather { .. } => "gather",
                                Action::NoOp => "noop",
                            };
                            *unmatched_kinds.entry(what.into()).or_default() += 1;
                        }
                    }
                }
                hist[k].record(tick as u32, &action, captured);
            }
            st.step(cmds);
        }
        eprintln!(
            "game {gi} {id}: {} seats, rows {}, {:?} {:?} kinds {:?} cands/tick {:.0} ({:.0}s)",
            seats.len(),
            out.nrows,
            tally,
            unmatched_kinds,
            kinds,
            ncands as f64 / tally.values().sum::<u64>().max(1) as f64,
            started.elapsed().as_secs_f64()
        );
    }
    out.x.flush().unwrap();
    out.rows.flush().unwrap();
    out.groups.flush().unwrap();
    Ok(())
}

fn seat_style(id: &str, arena: &str) -> Result<hexarena_sdk::Style, String> {
    hexarena_arena::entrants::load_all(std::path::Path::new(arena))
        .into_iter()
        .find(|e| e.id == id)
        .map(|e| e.style())
        .or_else(|| hexarena_sdk::style(id))
        .ok_or(format!("unknown bot {id}"))
}

/// DAgger: the policy plays real lobbies while a shadow copy of a teacher, fed the same
/// views, says what it would do; sampled states are labelled with the teacher's choice.
/// In blocks of `block` ticks the teacher takes over with probability `beta`, so the
/// states stay close to ones the teacher reaches too.
fn dagger(f: &BTreeMap<String, String>) -> Result<(), String> {
    use hexarena_sdk::brains::policy::{Model, Policy};
    use hexarena_sdk::{brains, Bot};
    let arena = f.get("arena").cloned().unwrap_or_else(|| "/workspace/data/arena".into());
    let model: Model = serde_json::from_slice(&std::fs::read(f.get("model").ok_or("--model")?).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let teachers: Vec<String> = f.get("teachers").map_or("burrow-v9,pangolin-v9", |s| s.as_str()).split(',').map(String::from).collect();
    let others: Vec<String> = f
        .get("others")
        .map_or("pangolin-v9,burrow-v9,pangolin-v9,ridgeline,relay", |s| s.as_str())
        .split(',')
        .map(String::from)
        .collect();
    let games: u64 = get(f, "games", 4);
    let seed0: u64 = get(f, "seed", 3_000_000);
    let every: usize = get(f, "every", 30);
    let beta: f64 = get(f, "beta", 0.3);
    let block: u32 = get(f, "block", 100);
    let k_kind: usize = get(f, "k", 4);
    let k_sib: usize = get(f, "sib", 8);
    let game_base: u32 = get(f, "game-base", 10_000);
    let k_hard: usize = get(f, "hard", 16);
    let prefix = f.get("out").ok_or("--out")?.clone();
    let open = |ext: &str| std::io::BufWriter::new(std::fs::File::create(format!("{prefix}.{ext}")).unwrap());
    let mut out = Out { x: open("x.f16"), rows: open("rows.f32"), groups: open("groups.u32"), nrows: 0 };
    let mut games_out = open("games.jsonl");
    let mut rng = Rng(seed0 ^ 0xDA66);
    let started = std::time::Instant::now();
    let (mut matched, mut missed, mut agree) = (0u64, 0u64, 0u64);
    let mut confusion = [[0u64; NK]; NK];
    for g in 0..games {
        let seed = seed0 + g;
        let n = 3 + others.len();
        let mut st = State::new(hexarena_arena::settings(n as u16, seed), (0..n).map(|i| format!("Player {}", i + 1)).collect())?;
        // Seats are shuffled so the policy doesn't always start in the same places.
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            order.swap(i, rng.below(i + 1));
        }
        let mut pols: Vec<Policy> = (0..3).map(|i| Policy::with_model(format!("p{i}"), model.clone())).collect();
        let mut shadows: Vec<Box<dyn Bot>> =
            (0..3).map(|i| brains::build(seat_style(&teachers[(i + g as usize) % teachers.len()], &arena).unwrap()).unwrap()).collect();
        let mut rest: Vec<Box<dyn Bot>> = others.iter().map(|o| brains::build(seat_style(o, &arena).unwrap()).unwrap()).collect();
        let mut teacher_on = [false; 3];
        let mut record: Vec<TickCommands> = vec![];
        let mut labels: Vec<(u32, u16, Action)> = vec![];
        while !st.is_over() {
            let mut cmds = vec![];
            for (seat, &role) in order.iter().enumerate() {
                let p = seat as PlayerId;
                if st.eliminated[seat] {
                    continue;
                }
                let view = st.view(p);
                let cmd = if role < 3 {
                    if view.tick % block == 0 {
                        teacher_on[role] = rng.unit() < beta;
                    }
                    let tcmd = shadows[role].act(&view);
                    let pol = &mut pols[role];
                    let hist = pol.hist;
                    pol.fz.build(&view, &hist);
                    let i = pol.choose();
                    let t = pol.fz.find(&tcmd.action);
                    let label = view.tick > 0 && rng.below(every) == 0;
                    if label {
                        labels.push((view.tick, p, tcmd.action.clone()));
                    }
                    match t {
                        Some(t) => {
                            matched += 1;
                            agree += (t == i) as u64;
                            confusion[pol.fz.cands[t].kind.index()][pol.fz.cands[i].kind.index()] += 1;
                            if label {
                                write_sample(&mut out, &pol.fz, t, k_kind, k_sib, Some((pol.scores(), k_hard)), &mut rng, [view.tick, 0, game_base + g as u32]);
                            }
                        }
                        None => missed += 1,
                    }
                    let (action, management) = if teacher_on[role] {
                        (tcmd.action.clone(), tcmd.management.clone())
                    } else {
                        let c = pol.fz.cands[i];
                        (pol.fz.action(&c), pol.fz.wall_links(&view))
                    };
                    let captured = match &action {
                        Action::Move { to, .. } => st.cell(*to).is_some_and(|c| c.owner != Some(p)),
                        _ => false,
                    };
                    pol.hist.record(view.tick, &action, captured);
                    PlayerCommand { management, action }
                } else {
                    rest[role - 3].act(&view)
                };
                cmds.push((p, cmd));
            }
            st.step(&cmds);
            record.push(cmds);
        }
        let policy_seats: Vec<usize> = (0..n).filter(|&seat| order[seat] < 3).collect();
        let line = serde_json::json!({ "seed": seed, "n": n, "policy_seats": policy_seats, "commands": record, "labels": labels });
        games_out.write_all(serde_json::to_string(&line).unwrap().as_bytes()).unwrap();
        games_out.write_all(b"\n").unwrap();
        games_out.flush().unwrap();
        let r = st.final_result();
        let places: Vec<f64> = (0..3).map(|role| r.place_f64(order.iter().position(|&x| x == role).unwrap() as PlayerId)).collect();
        eprintln!(
            "dagger game {g} (seed {seed}): policy places {places:?}, rows {}, matched {matched} missed {missed}, agreement {:.3} ({:.0}s)",
            out.nrows,
            agree as f64 / matched.max(1) as f64,
            started.elapsed().as_secs_f64()
        );
        let names: Vec<String> = (0..n).map(|seat| if order[seat] < 3 { "policy".into() } else if order[seat] >= 3 { others[order[seat] - 3].clone() } else { String::new() }).collect();
        for pl in &r.placements {
            eprintln!(
                "  place {} {:<12} land {:>4} buildings {:>4} troops {:>9} out at {:?}",
                pl.place_start, names[pl.player as usize], pl.land, pl.buildings, pl.troops, pl.eliminated_at
            );
        }
        out.x.flush().unwrap();
        out.rows.flush().unwrap();
        out.groups.flush().unwrap();
    }
    eprintln!("teacher kind (rows) x policy kind (columns): capN capE relay farm village quarry wall gather noop");
    for (k, row) in confusion.iter().enumerate() {
        eprintln!("  {:<8} {:?}", ["capN", "capE", "relay", "farm", "village", "quarry", "wall", "gather", "noop"][k], row);
    }
    Ok(())
}

/// Re-featurizes recorded DAgger games (`dagger`'s `.games.jsonl`) with the current
/// features: re-simulates each game from its seed and commands and writes a sample for
/// every teacher label.
fn relabel(f: &BTreeMap<String, String>) -> Result<(), String> {
    let prefix = f.get("out").ok_or("--out")?.clone();
    let k_kind: usize = get(f, "k", 3);
    let k_sib: usize = get(f, "sib", 6);
    let k_hard: usize = get(f, "hard", 16);
    let game_base: u32 = get(f, "game-base", 50_000);
    let scorer = match f.get("model") {
        Some(p) => {
            let m: hexarena_sdk::brains::policy::Model =
                serde_json::from_slice(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Some(hexarena_sdk::brains::policy::Folded::new(&m))
        }
        None => None,
    };
    let mut scores: Vec<f32> = vec![];
    let open = |ext: &str| std::io::BufWriter::new(std::fs::File::create(format!("{prefix}.{ext}")).unwrap());
    let mut out = Out { x: open("x.f16"), rows: open("rows.f32"), groups: open("groups.u32"), nrows: 0 };
    let mut rng = Rng(get(f, "seed", 5));
    let (mut matched, mut missed) = (0u64, 0u64);
    let mut gi = 0u32;
    for path in f.get("games").ok_or("--games a.games.jsonl,b.games.jsonl")?.split(',') {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        for line in text.lines() {
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let seed = v["seed"].as_u64().unwrap();
            let n = v["n"].as_u64().unwrap() as usize;
            let seats: Vec<u16> = serde_json::from_value(v["policy_seats"].clone()).map_err(|e| e.to_string())?;
            let commands: Vec<TickCommands> = serde_json::from_value(v["commands"].clone()).map_err(|e| e.to_string())?;
            let labels: Vec<(u32, u16, Action)> = serde_json::from_value(v["labels"].clone()).map_err(|e| e.to_string())?;
            let mut by_tick: BTreeMap<(u32, u16), Action> = BTreeMap::new();
            for (t, p, a) in labels {
                by_tick.insert((t, p), a);
            }
            let mut st = State::new(hexarena_arena::settings(n as u16, seed), (0..n).map(|i| format!("Player {}", i + 1)).collect())?;
            let mut fz: Vec<Featurizer> = seats.iter().map(|_| Featurizer::new()).collect();
            let mut hist: Vec<History> = seats.iter().map(|_| History::default()).collect();
            for (tick, cmds) in commands.iter().enumerate() {
                for (k, &seat) in seats.iter().enumerate() {
                    if st.eliminated[seat as usize] {
                        continue;
                    }
                    let action = cmds.iter().find(|(p, _)| *p == seat).map(|(_, c)| c.action.clone()).unwrap_or(Action::NoOp);
                    if tick == 0 {
                        // The first view carries the rules; featurize it so they are adopted.
                        fz[k].build(&st.view(seat), &hist[k]);
                    }
                    if let Some(label) = by_tick.get(&(tick as u32, seat)) {
                        let view = st.view(seat);
                        fz[k].build(&view, &hist[k]);
                        match fz[k].find(label) {
                            Some(i) => {
                                matched += 1;
                                let hard = scorer.as_ref().map(|m| {
                                    m.score_all(&fz[k].feats, &mut scores);
                                    (&scores[..], k_hard)
                                });
                                write_sample(&mut out, &fz[k], i, k_kind, k_sib, hard, &mut rng, [tick as u32, 0, game_base + gi]);
                            }
                            None => missed += 1,
                        }
                    }
                    let captured = match &action {
                        Action::Move { to, .. } => st.cell(*to).is_some_and(|c| c.owner != Some(seat)),
                        _ => false,
                    };
                    hist[k].record(tick as u32, &action, captured);
                }
                st.step(cmds);
            }
            eprintln!("relabelled game {gi} (seed {seed}): matched {matched} missed {missed}, rows {}", out.nrows);
            gi += 1;
        }
    }
    out.x.flush().unwrap();
    out.rows.flush().unwrap();
    out.groups.flush().unwrap();
    Ok(())
}

fn bench(f: &BTreeMap<String, String>) -> Result<(), String> {
    use hexarena_sdk::{roster, style, BotParams, Style};
    let games: u64 = get(f, "games", 2);
    let seed: u64 = get(f, "seed", 900);
    let others: Vec<String> = f
        .get("others")
        .map(|s| s.split(',').map(String::from).collect())
        .unwrap_or_else(|| ["ridgeline", "relay", "predator", "warden", "expander"].map(String::from).to_vec());
    let pol = Style {
        id: "policy".into(),
        name: "policy".into(),
        blurb: String::new(),
        params: BotParams { brain: "policy".into(), ..BotParams::default() },
        program: None,
    };
    let mut slowest = 0f64;
    let (mut total, mut moves) = (0f64, 0u64);
    let mut hist = vec![0u64; 12];
    for g in 0..games {
        let mut seats = vec![pol.clone(); 3];
        for o in &others {
            seats.push(style(o).unwrap_or_else(|| roster().remove(0)));
        }
        seats.truncate(8);
        let played = hexarena_arena::play(seed + g, &seats, u32::MAX)?;
        let r = played.result();
        let places: Vec<f64> = (0..3).map(|p| r.place_f64(p)).collect();
        for t in &played.move_time[..3] {
            slowest = slowest.max(t.slowest.as_secs_f64() * 1000.0);
            total += t.total.as_secs_f64() * 1000.0;
            moves += t.moves;
        }
        let _ = &mut hist;
        println!(
            "game {g}: final tick {} policy places {:?}; mean {:.3} ms, slowest so far {:.2} ms",
            r.final_tick,
            places,
            total / moves.max(1) as f64,
            slowest
        );
    }
    println!("{{\"moves\": {moves}, \"mean_ms\": {:.4}, \"max_ms\": {:.3}}}", total / moves.max(1) as f64, slowest);
    Ok(())
}

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

extern "C" {
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
}

/// CPU time this thread has used, in ms: unlike wall time it doesn't grow while the OS
/// runs other processes, so it shows a move's own cost on a loaded machine.
pub fn thread_cpu_ms() -> f64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    // SAFETY: CLOCK_THREAD_CPUTIME_ID (3) with a valid out-pointer.
    unsafe { clock_gettime(3, &mut ts) };
    ts.sec as f64 * 1000.0 + ts.nsec as f64 / 1e6
}

/// Plays one lobby with policy bots driven directly, timing featurization and scoring
/// separately and counting candidates, by phase of the game.
fn profile(f: &BTreeMap<String, String>) -> Result<(), String> {
    use hexarena_sdk::brains::policy::{Model, Policy};
    use hexarena_sdk::{brains, style, Bot};
    let seed: u64 = get(f, "seed", 901);
    let others = ["ridgeline", "relay", "predator", "warden", "expander"];
    let n = 8;
    let mut st = State::new(hexarena_arena::settings(n, seed), (0..n).map(|i| format!("P{i}")).collect())?;
    let mut pols: Vec<Policy> = (0..3).map(|i| Policy::with_model(format!("p{i}"), Model::embedded())).collect();
    let mut rest: Vec<Box<dyn Bot>> = others.iter().map(|o| brains::build(style(o).unwrap()).unwrap()).collect();
    let mut buckets = vec![(0f64, 0f64, 0u64, 0u64, 0f64); 10];
    let mut worst = (0f64, 0u32, 0usize);
    let mut cpu_max = 0f64;
    let mut walls: Vec<f64> = vec![];
    while !st.is_over() {
        let mut cmds = vec![];
        for p in 0..n {
            if st.eliminated[p as usize] {
                continue;
            }
            let view = st.view(p);
            let cmd = if (p as usize) < 3 {
                let pol = &mut pols[p as usize];
                let t0 = std::time::Instant::now();
                let c0 = thread_cpu_ms();
                let hist = pol.hist;
                pol.fz.build(&view, &hist);
                let t1 = t0.elapsed().as_secs_f64() * 1000.0;
                let i = pol.choose();
                let c = pol.fz.cands[i];
                let action = pol.fz.action(&c);
                let management = pol.fz.wall_links(&view);
                let t2 = t0.elapsed().as_secs_f64() * 1000.0;
                let cpu = thread_cpu_ms() - c0;
                cpu_max = cpu_max.max(cpu);
                walls.push(t2);
                pol.hist.record(view.tick, &action, matches!(c.kind, Kind::CapEnemy | Kind::CapNeutral));
                let b = &mut buckets[(view.tick / 1000).min(9) as usize];
                b.0 += t1;
                b.1 += t2 - t1;
                b.2 += pol.fz.cands.len() as u64;
                b.3 += 1;
                b.4 = b.4.max(t2);
                if t2 > worst.0 {
                    worst = (t2, view.tick, pol.fz.cands.len());
                }
                PlayerCommand { management, action }
            } else {
                rest[p as usize - 3].act(&view)
            };
            cmds.push((p, cmd));
        }
        st.step(&cmds);
    }
    for (k, b) in buckets.iter().enumerate() {
        let m = b.3.max(1) as f64;
        println!("t{}k: feat {:.3} ms, score {:.3} ms, cands {:.0}, max {:.2} ms", k, b.0 / m, b.1 / m, b.2 as f64 / m, b.4);
    }
    walls.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| walls[((walls.len() - 1) as f64 * p) as usize];
    println!("wall p50 {:.3} p99 {:.3} p99.9 {:.3} max {:.2} ms; thread-cpu max {:.3} ms", q(0.5), q(0.99), q(0.999), q(1.0), cpu_max);
    println!("worst {:.2} ms at tick {} with {} cands; result {:?}", worst.0, worst.1, worst.2, (0..3).map(|p| st.final_result().place_f64(p)).collect::<Vec<_>>());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let f = flags(args.get(1..).unwrap_or(&[]));
    let res = match args.first().map(String::as_str) {
        Some("extract") => extract(&f),
        Some("bench") => bench(&f),
        Some("es") => es::run(&f),
        Some("profile") => profile(&f),
        Some("dagger") => dagger(&f),
        Some("relabel") => relabel(&f),
        _ => Err("usage: policy-train <extract|bench|es> ...".into()),
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
