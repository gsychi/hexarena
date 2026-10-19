//! Two bots side by side: their head-to-head record, the games they shared and how
//! each one plays, in 1v1s and 8-player free-for-alls, under the current rules.

use super::{err, renamed, Ending, Shared, Standing, Status, Summary};
use crate::habits::Habits;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use hexarena_protocol::RulesConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Shared games listed, newest first.
const SHARED_GAMES: usize = 80;

#[derive(Deserialize)]
pub struct CompareQuery {
    a: String,
    b: String,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Format {
    Duel,
    Ffa,
    /// 3 to 7 players: only requested games.
    Other,
}

fn format(m: &Summary) -> Format {
    match m.seats.len() {
        2 => Format::Duel,
        8 => Format::Ffa,
        _ => Format::Other,
    }
}

/// The bot's best seat in a game (a lineup can seat it twice) and that seat's index.
fn seat_of<'a>(m: &'a Summary, bot: &str) -> Option<&'a Standing> {
    m.standings.iter().filter(|s| s.bot == bot).min_by_key(|s| (s.place_start, s.place_end))
}

fn place(s: &Standing) -> f64 {
    (s.place_start + s.place_end) as f64 / 2.0
}

#[derive(Default, Serialize)]
struct Record {
    games: usize,
    /// Games where `a` finished ahead of `b`, and the other way round.
    a_ahead: usize,
    b_ahead: usize,
    even: usize,
    /// Games one of them won outright.
    a_won: usize,
    b_won: usize,
}

#[derive(Default)]
struct Tally {
    games: usize,
    place: f64,
    wins: usize,
    /// Won before the tick cutoff.
    finishes: usize,
    land: f64,
    ms: Vec<f64>,
    ticks: f64,
    habit_games: usize,
    built: [f64; 4],
    with_wall: usize,
    first_walls: Vec<u32>,
    demolished: f64,
    captures: f64,
    rival_captures: f64,
    repelled: f64,
    lost: f64,
    sealed: f64,
    sealed_lost: f64,
}

impl Tally {
    fn add(&mut self, m: &Summary, s: &Standing, h: Option<&Habits>) {
        self.games += 1;
        self.place += place(s);
        let won = s.place_start == 1 && s.place_end == 1;
        self.wins += won as usize;
        self.finishes += (won && m.ending != Some(Ending::Cutoff)) as usize;
        self.land += s.land as f64;
        self.ms.extend(s.ms_per_move);
        self.ticks += m.final_tick.unwrap_or(m.max_ticks) as f64;
        if let Some(h) = h {
            self.habit_games += 1;
            for k in 0..4 {
                self.built[k] += h.built[k] as f64;
            }
            if let Some(t) = h.first_wall {
                self.with_wall += 1;
                self.first_walls.push(t);
            }
            self.demolished += h.demolished as f64;
            self.captures += h.captures as f64;
            self.rival_captures += h.rival_captures as f64;
            self.repelled += h.repelled as f64;
            self.lost += h.lost as f64;
            self.sealed += h.sealed as f64;
            self.sealed_lost += h.sealed_lost as f64;
        }
    }

    fn report(mut self) -> serde_json::Value {
        if self.games == 0 {
            return serde_json::json!({ "games": 0 });
        }
        let g = self.games as f64;
        let hg = self.habit_games.max(1) as f64;
        let per = |x: f64| (self.habit_games > 0).then(|| x / hg);
        self.first_walls.sort_unstable();
        let attacks = self.captures + self.repelled;
        serde_json::json!({
            "games": self.games,
            "avg_place": self.place / g,
            "win_pct": 100.0 * self.wins as f64 / g,
            "finish_pct": 100.0 * self.finishes as f64 / g,
            "avg_land": self.land / g,
            "avg_ticks": self.ticks / g,
            "ms_per_move": (!self.ms.is_empty()).then(|| self.ms.iter().sum::<f64>() / self.ms.len() as f64),
            "habit_games": self.habit_games,
            "farms": per(self.built[0]),
            "villages": per(self.built[1]),
            "quarries": per(self.built[2]),
            "walls": per(self.built[3]),
            "wall_pct": (self.habit_games > 0).then(|| 100.0 * self.with_wall as f64 / hg),
            "first_wall": self.first_walls.get(self.first_walls.len() / 2),
            "demolished": per(self.demolished),
            "captures": per(self.captures),
            "rival_captures": per(self.rival_captures),
            "attack_success_pct": (attacks > 0.0).then(|| 100.0 * self.captures / attacks),
            "lost": per(self.lost),
            "sealed": per(self.sealed),
            "sealed_lost": per(self.sealed_lost),
        })
    }
}

pub async fn compare(State(app): State<Shared>, Query(q): Query<CompareQuery>) -> Response {
    let catalog = app.catalog();
    for id in [&q.a, &q.b] {
        if !catalog.knows(id) {
            return err(StatusCode::NOT_FOUND, format!("unknown bot '{id}'"));
        }
    }
    if q.a == q.b {
        return err(StatusCode::BAD_REQUEST, "pick two different bots");
    }
    let current = RulesConfig::default().version;
    let matches = app.matches.lock().unwrap();
    let mut games: Vec<&Summary> = matches
        .values()
        .filter(|m| m.status == Status::Done && m.rules_version == current)
        .filter(|m| m.seats.iter().any(|s| s.bot == q.a || s.bot == q.b))
        .collect();
    games.sort_by(|x, y| y.created_at.cmp(&x.created_at).then(y.id.cmp(&x.id)));

    let mut h2h: BTreeMap<Format, Record> = BTreeMap::new();
    let mut shared = vec![];
    let mut tally: [BTreeMap<Format, Tally>; 2] = Default::default();
    let mut games_total = 0;
    let mut games_with_habits = 0;
    for m in &games {
        let habits = app.habits.get(&m.id);
        games_total += 1;
        games_with_habits += habits.is_some() as usize;
        let fmt = format(m);
        let seats = [seat_of(m, &q.a), seat_of(m, &q.b)];
        for (side, s) in seats.iter().enumerate() {
            if let (Some(s), Format::Duel | Format::Ffa) = (s, fmt) {
                let h = habits.as_ref().and_then(|h| h.get(s.player as usize));
                tally[side].entry(fmt).or_default().add(m, s, h);
            }
        }
        let [Some(sa), Some(sb)] = seats else { continue };
        let (pa, pb) = (place(sa), place(sb));
        let r = h2h.entry(fmt).or_default();
        r.games += 1;
        if pa < pb {
            r.a_ahead += 1;
        } else if pb < pa {
            r.b_ahead += 1;
        } else {
            r.even += 1;
        }
        r.a_won += (sa.place_start == 1 && sa.place_end == 1) as usize;
        r.b_won += (sb.place_start == 1 && sb.place_end == 1) as usize;
        if shared.len() < SHARED_GAMES {
            let m = renamed(m, &catalog);
            shared.push(serde_json::json!({
                "id": m.id, "created_at": m.created_at, "players": m.seats.len(), "map": m.map,
                "auto": m.auto, "ending": m.ending, "final_tick": m.final_tick, "winner": m.winner,
                "a": { "place_start": sa.place_start, "place_end": sa.place_end, "land": sa.land },
                "b": { "place_start": sb.place_start, "place_end": sb.place_end, "land": sb.land },
            }));
        }
    }
    let shared_total: usize = h2h.values().map(|r| r.games).sum();
    drop(matches);
    let [ta, tb] = tally;
    let report = |t: BTreeMap<Format, Tally>| -> serde_json::Value {
        t.into_iter().map(|(f, t)| (serde_json::to_value(f).unwrap().as_str().unwrap().to_string(), t.report())).collect()
    };
    let bot = |id: &str| serde_json::json!({ "id": id, "name": catalog.display_name(id), "retired": catalog.retired.contains(id) });
    Json(serde_json::json!({
        "rules_version": current,
        "a": bot(&q.a),
        "b": bot(&q.b),
        "head_to_head": h2h,
        "shared_total": shared_total,
        "shared": shared,
        "tendencies": { "a": report(ta), "b": report(tb) },
        "coverage": { "games": games_total, "with_habits": games_with_habits },
    }))
    .into_response()
}
