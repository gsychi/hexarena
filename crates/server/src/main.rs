//! Hexarena game server: creates CPU matches on the authoritative engine, stores
//! replays on disk, and serves match summaries, viewer documents and a leaderboard.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use hexarena_arena::{play, seat_names, viewer_json};
use hexarena_protocol::{PlayerId, RulesConfig};
use hexarena_sdk::Style;
use serde::{Deserialize, Serialize};
use catalog::Catalog;
use standings::{standings, SeatResult, K, RATED_GAMES, START_ELO};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use tower_http::services::{ServeDir, ServeFile};

mod catalog;
mod chat;
mod notes;
mod standings;

const MAX_PLAYERS: usize = 8;
const MAX_BATCH: u32 = 20;
/// Refuse new games while this many are still queued or running.
const MAX_RUNNING: usize = 100;
/// Most roster CPU seats in each self-play game; arena entrants fill the rest.
const AUTOPLAY_FIELD: usize = 2;
/// Most entrant seats one author gets in a self-play game.
const AUTOPLAY_PER_AUTHOR: usize = 2;
/// Arena entrants stay scheduled while they rank in this many places of the standings.
const ACTIVE_TOP: usize = 20;
/// Self-play games kept running at once, unless `HEXARENA_AUTOPLAY_PARALLEL` says otherwise.
const AUTOPLAY_PARALLEL: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Running,
    Done,
    Failed,
}

/// How a finished match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Ending {
    /// One player owns every traversable hex.
    Conquest,
    /// Everyone else was eliminated, with neutral land still unclaimed.
    LastStanding,
    /// The tick cutoff was reached first.
    Cutoff,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Seat {
    bot: String,
    name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Standing {
    player: PlayerId,
    name: String,
    bot: String,
    place_start: u16,
    place_end: u16,
    points: u32,
    normalized: f64,
    land: u32,
    /// Average time the bot took per move in this game (absent before timing existed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ms_per_move: Option<f64>,
    /// Its slowest single move in this game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slowest_ms: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Summary {
    id: String,
    created_at: u64,
    /// Games from before v0.1 have no version recorded.
    #[serde(default = "legacy_rules")]
    rules_version: String,
    #[serde(default)]
    ending: Option<Ending>,
    status: Status,
    error: Option<String>,
    seed: u64,
    max_ticks: u32,
    seats: Vec<Seat>,
    final_tick: Option<u32>,
    winner: Option<String>,
    standings: Vec<Standing>,
    /// Scheduled by the self-playing arena rather than requested by someone.
    #[serde(default)]
    auto: bool,
}

#[derive(Deserialize)]
struct CreateMatch {
    bots: Vec<String>,
    seed: Option<u64>,
    count: Option<u32>,
}

struct AppState {
    dir: PathBuf,
    arena_dir: PathBuf,
    matches: Mutex<BTreeMap<String, Summary>>,
    next_id: Mutex<u64>,
    workers: Semaphore,
    autoplay_parallel: usize,
    autoplay: bool,
    notes: notes::Notebook,
    chat: chat::Chat,
}

type Shared = Arc<AppState>;

fn legacy_rules() -> String {
    "v0".into()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// `s` with every bot that has one seat called by its current name, so renamed bots
/// read the same in old games. Repeated bots keep their numbered seat names.
fn renamed(s: &Summary, catalog: &Catalog) -> Summary {
    let mut s = s.clone();
    let once = |bot: &str| s.seats.iter().filter(|x| x.bot == bot).count() == 1;
    let names: Vec<Option<String>> = s.seats.iter().map(|x| (once(&x.bot) && catalog.knows(&x.bot)).then(|| catalog.display_name(&x.bot))).collect();
    for (seat, name) in s.seats.iter_mut().zip(&names) {
        if let Some(n) = name {
            if s.winner.as_deref() == Some(seat.name.as_str()) {
                s.winner = Some(n.clone());
            }
            seat.name = n.clone();
        }
    }
    for st in &mut s.standings {
        if let Some(Some(n)) = names.get(st.player as usize) {
            st.name = n.clone();
        }
    }
    s
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

impl AppState {
    fn path(&self, id: &str, kind: &str) -> PathBuf {
        self.dir.join(format!("{id}.{kind}.json"))
    }

    fn save(&self, s: &Summary) {
        let _ = std::fs::write(
            self.path(&s.id, "summary"),
            serde_json::to_vec_pretty(s).unwrap(),
        );
        self.matches.lock().unwrap().insert(s.id.clone(), s.clone());
    }

    fn catalog(&self) -> Catalog {
        Catalog::load(&self.arena_dir)
    }

    fn running(&self) -> usize {
        self.matches.lock().unwrap().values().filter(|m| m.status == Status::Running).count()
    }

    fn load(dir: PathBuf, arena_dir: PathBuf) -> AppState {
        std::fs::create_dir_all(&dir).expect("create data dir");
        let mut matches = BTreeMap::new();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".summary.json") {
                continue;
            }
            if let Ok(mut s) =
                serde_json::from_slice::<Summary>(&std::fs::read(entry.path()).unwrap_or_default())
            {
                if s.status == Status::Running {
                    s.status = Status::Failed;
                    s.error = Some("server restarted before the match finished".into());
                }
                matches.insert(s.id.clone(), s);
            }
        }
        let next = matches
            .keys()
            .filter_map(|k: &String| k.trim_start_matches('m').parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        AppState {
            dir,
            arena_dir,
            matches: Mutex::new(matches),
            next_id: Mutex::new(next),
            workers: Semaphore::new(workers),
            autoplay_parallel: AUTOPLAY_PARALLEL,
            autoplay: false,
            notes: notes::Notebook::new(),
            chat: chat::Chat::new(),
        }
    }
}

fn run_match(app: &AppState, mut summary: Summary, styles: Vec<Style>) -> Summary {
    let outcome = play(summary.seed, &styles, summary.max_ticks).and_then(|played| {
        let replay = played.replay();
        let viewer = viewer_json(&replay)?;
        std::fs::write(
            app.path(&summary.id, "replay"),
            serde_json::to_vec(&replay).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        std::fs::write(
            app.path(&summary.id, "viewer"),
            serde_json::to_vec(&viewer).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        let st = &played.state;
        let ending = match st.conqueror() {
            Some(p) => {
                let passable = st.terrain.iter().flatten().filter(|t| t.passable()).count();
                if st.hex_counts()[p as usize] as usize == passable {
                    Ending::Conquest
                } else {
                    Ending::LastStanding
                }
            }
            None => Ending::Cutoff,
        };
        Ok((played.result(), ending, played.move_time))
    });
    match outcome {
        Ok((result, ending, move_time)) => {
            summary.status = Status::Done;
            summary.ending = Some(ending);
            summary.final_tick = Some(result.final_tick);
            summary.winner = result
                .winner()
                .map(|p| summary.seats[p as usize].name.clone());
            summary.standings = result
                .placements
                .iter()
                .map(|p| Standing {
                    player: p.player,
                    name: summary.seats[p.player as usize].name.clone(),
                    bot: summary.seats[p.player as usize].bot.clone(),
                    place_start: p.place_start,
                    place_end: p.place_end,
                    points: p.points,
                    normalized: result.normalized(p.player),
                    land: p.land,
                    ms_per_move: move_time[p.player as usize].ms_per_move(),
                    slowest_ms: Some(move_time[p.player as usize].slowest.as_secs_f64() * 1000.0)
                        .filter(|_| move_time[p.player as usize].moves > 0),
                })
                .collect();
        }
        Err(e) => {
            summary.status = Status::Failed;
            summary.error = Some(e);
        }
    }
    summary
}

async fn bots(State(app): State<Shared>) -> Json<serde_json::Value> {
    Json(serde_json::json!(app.catalog().bots()))
}

#[derive(Deserialize)]
struct ListQuery {
    /// Newest games to return; all of them when absent.
    limit: Option<usize>,
}

/// Games newest first; `X-Total-Count` carries how many there are in all.
async fn list(State(app): State<Shared>, Query(q): Query<ListQuery>) -> Response {
    let catalog = app.catalog();
    let matches = app.matches.lock().unwrap();
    let total = matches.len();
    let mut v: Vec<&Summary> = matches.values().collect();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    v.truncate(q.limit.unwrap_or(usize::MAX));
    let v: Vec<Summary> = v.into_iter().map(|m| renamed(m, &catalog)).collect();
    ([("x-total-count", total.to_string())], Json(v)).into_response()
}

async fn create(State(app): State<Shared>, Json(req): Json<CreateMatch>) -> Response {
    if req.bots.len() < 2 || req.bots.len() > MAX_PLAYERS {
        return err(
            StatusCode::BAD_REQUEST,
            format!("pick 2 to {MAX_PLAYERS} CPU players"),
        );
    }
    let catalog = app.catalog();
    let styles: Result<Vec<Style>, String> = req
        .bots
        .iter()
        .map(|id| catalog.resolve(id).ok_or_else(|| format!("unknown bot '{id}'")))
        .collect();
    let styles = match styles {
        Ok(s) => s,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let count = req.count.unwrap_or(1).clamp(1, MAX_BATCH);
    if app.running() + count as usize > MAX_RUNNING {
        return err(
            StatusCode::TOO_MANY_REQUESTS,
            "the arena is busy, try again once some games finish",
        );
    }
    let base_seed = req.seed.unwrap_or_else(random_seed);
    let created: Vec<Summary> = (0..count)
        .map(|i| launch(&app, &styles, base_seed.wrapping_add(i as u64), false))
        .collect();
    (StatusCode::CREATED, Json(created)).into_response()
}

fn random_seed() -> u64 {
    now_ms().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 16
}

/// Records a new match and plays it on a worker in the background.
fn launch(app: &Shared, styles: &[Style], seed: u64, auto: bool) -> Summary {
    let names = seat_names(styles);
    let id = {
        let mut n = app.next_id.lock().unwrap();
        let id = format!("m{:05}", *n);
        *n += 1;
        id
    };
    let summary = Summary {
        id,
        created_at: now_ms(),
        rules_version: RulesConfig::default().version,
        ending: None,
        status: Status::Running,
        error: None,
        seed,
        max_ticks: RulesConfig::default().max_ticks,
        seats: styles
            .iter()
            .zip(&names)
            .map(|(s, n)| Seat { bot: s.id.clone(), name: n.clone() })
            .collect(),
        final_tick: None,
        winner: None,
        standings: vec![],
        auto,
    };
    app.save(&summary);
    let (app, styles, queued) = (app.clone(), styles.to_vec(), summary.clone());
    tokio::spawn(async move {
        let _permit = app.workers.acquire().await.unwrap();
        let worker = app.clone();
        let done = tokio::task::spawn_blocking(move || run_match(&worker, queued, styles))
            .await
            .expect("match task panicked");
        app.save(&done);
    });
    summary
}

/// Seats for the next self-play game, always [`MAX_PLAYERS`] when there are enough bots:
/// up to [`AUTOPLAY_FIELD`] of the least-played roster CPUs that aren't retired, then
/// the active entrants with the fewest arena games, at most [`AUTOPLAY_PER_AUTHOR`] from
/// one author so a prolific author's versions don't mostly play each other, topped up
/// past that cap when too few authors are active. Ties break at random; seat order is
/// shuffled.
fn autoplay_lineup(app: &AppState, catalog: &Catalog, seed: u64) -> Vec<Style> {
    let rules = RulesConfig::default().version;
    let mut played: BTreeMap<String, usize> = BTreeMap::new();
    for m in app.matches.lock().unwrap().values() {
        if m.status != Status::Failed && m.rules_version == rules {
            for seat in &m.seats {
                *played.entry(seat.bot.clone()).or_default() += 1;
            }
        }
    }
    let mut rng = seed | 1;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let games = |id: &str| played.get(id).copied().unwrap_or(0);
    let mut field: Vec<(usize, u64, Style)> = catalog.active_field().into_iter().map(|s| (games(&s.id), next(), s)).collect();
    field.sort_by_key(|(n, tie, _)| (*n, *tie));
    let mut seats: Vec<Style> = field.into_iter().map(|(_, _, s)| s).take(AUTOPLAY_FIELD).collect();
    let mut entrants: Vec<(usize, u64, &hexarena_arena::entrants::Entrant)> =
        catalog.active_entrants().map(|e| (games(&e.id), next(), e)).collect();
    entrants.sort_by_key(|&(n, tie, _)| (n, tie));
    let mut per_author: BTreeMap<&str, usize> = BTreeMap::new();
    let mut picked: Vec<&str> = vec![];
    for &(_, _, e) in &entrants {
        let n = per_author.entry(e.author.as_str()).or_default();
        if seats.len() < MAX_PLAYERS && *n < AUTOPLAY_PER_AUTHOR {
            *n += 1;
            picked.push(&e.id);
            seats.push(e.style());
        }
    }
    for &(_, _, e) in &entrants {
        if seats.len() >= MAX_PLAYERS {
            break;
        }
        if !picked.contains(&e.id.as_str()) {
            seats.push(e.style());
        }
    }
    for i in (1..seats.len()).rev() {
        seats.swap(i, (next() % (i as u64 + 1)) as usize);
    }
    seats
}

/// The self-playing arena: keeps `autoplay_parallel` 8-player games going.
async fn autoplay(app: Shared) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if app.running() >= app.autoplay_parallel {
            continue;
        }
        retire_outside_top(&app, &app.catalog());
        let catalog = app.catalog();
        let seed = random_seed();
        let lineup = autoplay_lineup(&app, &catalog, seed);
        launch(&app, &lineup, seed, true);
    }
}

async fn arena_info(State(app): State<Shared>) -> Json<serde_json::Value> {
    let catalog = app.catalog();
    let matches = app.matches.lock().unwrap();
    let entrants: Vec<serde_json::Value> = catalog
        .entrants
        .iter()
        .map(|e| {
            let games = matches
                .values()
                .filter(|m| m.status == Status::Done && m.seats.iter().any(|s| s.bot == e.id))
                .count();
            serde_json::json!({
                "id": e.id, "name": e.name, "author": e.author, "note": e.note,
                "created_at": e.created_at, "retired": e.retired, "retired_reason": e.retired_reason, "games": games,
                "brain": if e.params.brain.is_empty() { "expander" } else { e.params.brain.as_str() },
                "own_build": e.program.is_some(),
                "parents": e.parents,
            })
        })
        .collect();
    Json(serde_json::json!({
        "autoplay": app.autoplay,
        "top": ACTIVE_TOP,
        "trial_games": RATED_GAMES,
        "entrants": entrants,
        "retired": catalog.retired_reasons(),
    }))
}

async fn list_notes(State(app): State<Shared>, Query(f): Query<notes::Filter>) -> Json<Vec<notes::Note>> {
    Json(app.notes.list(&app.arena_dir, &f))
}

async fn add_note(State(app): State<Shared>, Json(new): Json<notes::NewNote>) -> Response {
    match app.notes.add(&app.arena_dir, new, now_ms()) {
        Ok(n) => (StatusCode::CREATED, Json(n)).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

/// Authors that aren't agents designing bots: the lead's own notes and server logs.
fn is_designer(author: &str) -> bool {
    author.starts_with("agent:") && !author.starts_with("agent:lead")
}

/// Everyone designing bots, most recently seen first: what they have sent in, how their
/// best bot is doing, what they last wrote in the notebook, and their message thread.
async fn agents(State(app): State<Shared>) -> Json<Vec<serde_json::Value>> {
    let catalog = app.catalog();
    let board = board(&app, &catalog);
    let models = hexarena_arena::entrants::author_models(&app.arena_dir);
    let notes = app.notes.all(&app.arena_dir);
    let messages = app.chat.all(&app.arena_dir);
    let statuses = app.chat.statuses(&app.arena_dir);
    let mut ids: std::collections::BTreeSet<&str> = catalog.entrants.iter().map(|e| e.author.as_str()).collect();
    ids.extend(notes.iter().map(|n| n.author.as_str()));
    ids.extend(messages.iter().map(|m| m.agent.as_str()));
    ids.extend(statuses.keys().map(String::as_str));
    let mut out: Vec<(u64, serde_json::Value)> = ids
        .into_iter()
        .filter(|a| is_designer(a))
        .map(|a| {
            let bots: Vec<&hexarena_arena::entrants::Entrant> = catalog.entrants.iter().filter(|e| e.author == a).collect();
            let latest = bots.iter().max_by_key(|e| e.created_at);
            let best = board.rows.iter().find(|r| bots.iter().any(|e| e.id == r.bot));
            let mine: Vec<&notes::Note> = notes.iter().filter(|n| n.author == a).collect();
            let thread: Vec<&chat::Message> = messages.iter().filter(|m| m.agent == a).collect();
            let replied = thread.iter().rev().find(|m| m.from == a);
            let last_seen = [
                latest.map(|e| e.created_at),
                mine.last().map(|n| n.at),
                replied.map(|m| m.at),
                statuses.get(a).map(|s| s.at),
            ]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(0);
            let unanswered = thread.iter().rev().take_while(|m| m.from != a).count();
            let v = serde_json::json!({
                "id": a,
                "model": models.get(a),
                "bots": bots.len(),
                "active_bots": bots.iter().filter(|e| !e.retired).count(),
                "latest_bot": latest.map(|e| serde_json::json!({"id": e.id, "name": e.name, "at": e.created_at, "retired": e.retired})),
                "best_bot": best.map(|r| serde_json::json!({"id": r.bot, "name": r.name, "elo": r.elo, "rank": r.rank})),
                "notes": mine.len(),
                "last_note": mine.last().map(|n| serde_json::json!({"n": n.n, "at": n.at, "kind": n.kind, "text": n.text})),
                "last_seen": last_seen,
                "messages": thread.len(),
                "last_message": thread.last(),
                "unanswered": unanswered,
                "status": statuses.get(a),
            });
            (last_seen, v)
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    Json(out.into_iter().map(|(_, v)| v).collect())
}

async fn set_status(State(app): State<Shared>, Path(agent): Path<String>, Json(new): Json<chat::NewStatus>) -> Response {
    if !is_designer(&agent) {
        return err(StatusCode::BAD_REQUEST, "status belongs to an agent, e.g. agent:sol");
    }
    match app.chat.set_status(&app.arena_dir, &agent, new, now_ms()) {
        Ok(s) => (StatusCode::CREATED, Json(s)).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

/// Newest items an agent's activity feed keeps.
const ACTIVITY_ITEMS: usize = 300;

/// One agent's work and conversation as a single timeline, oldest first: the bots it
/// sent in (with their hypothesis and how they are doing), its notebook notes, and its
/// message thread.
async fn activity(State(app): State<Shared>, Path(agent): Path<String>) -> Json<serde_json::Value> {
    let catalog = app.catalog();
    let board = board(&app, &catalog);
    let row = |id: &str| board.rows.iter().chain(&board.retired_rows).find(|r| r.bot == id);
    let mut items: Vec<(u64, serde_json::Value)> = vec![];
    for e in catalog.entrants.iter().filter(|e| e.author == agent) {
        let r = row(&e.id);
        items.push((e.created_at, serde_json::json!({
            "type": "bot", "at": e.created_at, "id": e.id, "name": e.name, "note": e.note,
            "parents": e.parents, "retired": e.retired, "retired_reason": e.retired_reason,
            "elo": r.map(|r| r.elo), "rank": r.filter(|_| !e.retired).map(|r| r.rank), "games": r.map_or(0, |r| r.games),
        })));
    }
    for n in app.notes.all(&app.arena_dir).into_iter().filter(|n| n.author == agent) {
        items.push((n.at, serde_json::json!({
            "type": "note", "at": n.at, "n": n.n, "kind": n.kind, "text": n.text, "matches": n.matches, "bots": n.bots,
        })));
    }
    for m in app.chat.thread(&app.arena_dir, &agent) {
        items.push((m.at, serde_json::json!({ "type": "message", "at": m.at, "n": m.n, "from": m.from, "text": m.text })));
    }
    items.sort_by_key(|(at, _)| *at);
    let skip = items.len().saturating_sub(ACTIVITY_ITEMS);
    Json(serde_json::json!({
        "agent": agent,
        "status": app.chat.statuses(&app.arena_dir).get(&agent),
        "items": items.into_iter().skip(skip).map(|(_, v)| v).collect::<Vec<_>>(),
    }))
}

async fn thread(State(app): State<Shared>, Path(agent): Path<String>) -> Json<Vec<chat::Message>> {
    Json(app.chat.thread(&app.arena_dir, &agent))
}

async fn say(State(app): State<Shared>, Path(agent): Path<String>, Json(new): Json<chat::NewMessage>) -> Response {
    if !is_designer(&agent) {
        return err(StatusCode::BAD_REQUEST, "messages go to an agent, e.g. agent:sol");
    }
    match app.chat.add(&app.arena_dir, &agent, new, now_ms()) {
        Ok(m) => (StatusCode::CREATED, Json(m)).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn one(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let catalog = app.catalog();
    match app.matches.lock().unwrap().get(&id) {
        Some(s) => Json(renamed(s, &catalog)).into_response(),
        None => err(StatusCode::NOT_FOUND, "no such match"),
    }
}

async fn artifact(app: Shared, id: String, kind: &str) -> Response {
    if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return err(StatusCode::BAD_REQUEST, "bad id");
    }
    match tokio::fs::read(app.path(&id, kind)).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
        Err(_) => err(StatusCode::NOT_FOUND, "match not finished or missing"),
    }
}

async fn viewer(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    artifact(app, id, "viewer").await
}

async fn replay(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    artifact(app, id, "replay").await
}

/// Standings only count arena games played under the current rules, since the rules
/// and the CPUs change together.
async fn leaderboard(State(app): State<Shared>) -> Json<serde_json::Value> {
    let catalog = app.catalog();
    let board = board(&app, &catalog);
    Json(serde_json::json!({
        "games": board.games,
        "rules_version": RulesConfig::default().version,
        "start_elo": START_ELO,
        "k": K,
        "top": ACTIVE_TOP,
        "rated_games": RATED_GAMES,
        "retired": catalog.retired_reasons(),
        "bots": board.rows,
        "retired_bots": board.retired_rows,
    }))
}

/// Points kept per rating curve on a profile; longer curves are thinned evenly.
const PROFILE_POINTS: usize = 400;
/// Newest games listed on a profile.
const PROFILE_GAMES: usize = 300;

/// One bot's page under the current rules: who it is, its rating curve and its arena
/// games, newest first. Earlier rules versions are left out, since ratings restart with
/// every version and old results describe a different game.
async fn profile(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let catalog = app.catalog();
    let Some(info) = catalog.bots().into_iter().find(|b| b["id"] == id.as_str()) else {
        return err(StatusCode::NOT_FOUND, format!("unknown bot '{id}'"));
    };
    let current = RulesConfig::default().version;
    let games: Vec<serde_json::Value> = {
        let matches = app.matches.lock().unwrap();
        let mut mine: Vec<&Summary> = matches
            .values()
            .filter(|m| m.rules_version == current && m.seats.iter().any(|s| s.bot == id))
            .collect();
        mine.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        mine.iter()
            .take(PROFILE_GAMES)
            .map(|m| {
                let seat = m.standings.iter().filter(|s| s.bot == id).min_by_key(|s| s.place_start);
                serde_json::json!({
                    "id": m.id, "created_at": m.created_at, "rules_version": m.rules_version,
                    "status": m.status, "ending": m.ending, "final_tick": m.final_tick,
                    "players": m.seats.len(), "winner": m.winner, "auto": m.auto,
                    "place_start": seat.map(|s| s.place_start), "place_end": seat.map(|s| s.place_end),
                    "ms_per_move": seat.and_then(|s| s.ms_per_move),
                    "opponents": m.seats.iter().filter(|s| s.bot != id).map(|s| catalog.display_name(&s.bot)).collect::<Vec<_>>(),
                })
            })
            .collect()
    };
    let b = board(&app, &catalog);
    let rank = b.rows.iter().find(|r| r.bot == id).map(|r| r.rank);
    let row = b.rows.into_iter().chain(b.retired_rows).find(|r| r.bot == id);
    let curves: Vec<serde_json::Value> = row
        .iter()
        .map(|row| {
            let step = row.timeline.len().div_ceil(PROFILE_POINTS).max(1);
            let last = row.timeline.len().saturating_sub(1);
            let points: Vec<(u64, f64)> = row
                .timeline
                .iter()
                .enumerate()
                .filter(|(i, _)| i % step == 0 || *i == last)
                .map(|(_, &(g, elo))| (b.times[g], elo))
                .collect();
            serde_json::json!({
                "rules_version": current, "games": row.games, "elo": row.elo, "peak_elo": row.peak_elo, "points": points,
            })
        })
        .collect();
    let kin = |ids: Vec<&String>| -> Vec<serde_json::Value> {
        ids.into_iter()
            .map(|p| serde_json::json!({ "id": p, "name": catalog.display_name(p), "retired": catalog.retired.contains(p.as_str()) }))
            .collect()
    };
    let parents = kin(catalog.entrants.iter().find(|e| e.id == id).map(|e| e.parents.iter().collect()).unwrap_or_default());
    let children = kin(catalog.entrants.iter().filter(|e| e.parents.contains(&id)).map(|e| &e.id).collect());
    Json(serde_json::json!({
        "bot": info,
        "parents": parents,
        "children": children,
        "rules_version": current,
        "start_elo": START_ELO,
        "rated_games": RATED_GAMES,
        "standing": row.map(|row| serde_json::json!({ "rank": rank, "row": row })),
        "curves": curves,
        "games": games,
    }))
    .into_response()
}

/// Each seat's Elo going into a game and, once it is rated, after it. A game still
/// playing shows every bot's current rating; one from older rules has none.
async fn match_elo(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    let Some(m) = app.matches.lock().unwrap().get(&id).cloned() else {
        return err(StatusCode::NOT_FOUND, "no such match");
    };
    let current = m.rules_version == RulesConfig::default().version;
    let catalog = app.catalog();
    let b = board(&app, &catalog);
    let game = b.match_ids.iter().position(|g| *g == id);
    let rows: BTreeMap<&str, &standings::BotStanding> = b.rows.iter().chain(&b.retired_rows).map(|r| (r.bot.as_str(), r)).collect();
    let mut seen = std::collections::BTreeSet::new();
    let bots: Vec<serde_json::Value> = m
        .seats
        .iter()
        .filter(|s| seen.insert(s.bot.clone()))
        .map(|s| {
            let row = rows.get(s.bot.as_str());
            let (before, after) = match (row, game) {
                (Some(r), Some(g)) => match r.timeline.iter().position(|&(gi, _)| gi == g) {
                    Some(i) => (Some(if i == 0 { START_ELO } else { r.timeline[i - 1].1 }), Some(r.timeline[i].1)),
                    None => (None, None),
                },
                (Some(r), None) if current && m.status == Status::Running => (Some(r.elo), None),
                (None, None) if current && m.status == Status::Running => (Some(START_ELO), None),
                _ => (None, None),
            };
            serde_json::json!({ "bot": s.bot, "name": catalog.display_name(&s.bot), "before": before, "after": after })
        })
        .collect();
    Json(serde_json::json!({ "rules_version": m.rules_version, "rated": game.is_some(), "bots": bots })).into_response()
}

struct Board {
    games: usize,
    /// Active bots, ranked among themselves.
    rows: Vec<standings::BotStanding>,
    /// Retired bots, kept out of the standings. Ratings still count every game.
    retired_rows: Vec<standings::BotStanding>,
    /// When each rated game finished, indexed like `BotStanding::timeline`.
    times: Vec<u64>,
    /// Each rated game's match id, indexed the same way.
    match_ids: Vec<String>,
}

/// Every game counts toward its seats' ratings, but only bots that still exist get a
/// row: old league generations (`g19`, `champion`) still played in early v0.6 games.
fn board(app: &AppState, catalog: &Catalog) -> Board {
    let rules = RulesConfig::default().version;
    let matches = app.matches.lock().unwrap();
    let mut done: Vec<&Summary> = matches
        .values()
        .filter(|m| m.status == Status::Done && m.rules_version == rules)
        .collect();
    done.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    let games: Vec<Vec<SeatResult>> = done
        .iter()
        .map(|m| {
            m.standings
                .iter()
                .map(|s| SeatResult {
                    bot: &s.bot,
                    place: (s.place_start + s.place_end) as f64 / 2.0,
                    normalized: s.normalized,
                    won: s.place_start == 1 && s.place_end == 1,
                    players: m.standings.len(),
                    ended: matches!(m.ending, Some(Ending::Conquest | Ending::LastStanding)),
                    ms_per_move: s.ms_per_move,
                    slowest_ms: s.slowest_ms,
                })
                .collect()
        })
        .collect();
    let times: Vec<u64> = done.iter().map(|m| m.created_at).collect();
    let match_ids: Vec<String> = done.iter().map(|m| m.id.clone()).collect();
    let (mut rows, retired_rows): (Vec<_>, Vec<_>) = standings(&games, |id| catalog.display_name(id))
        .into_iter()
        .filter(|r| catalog.knows(&r.bot))
        .partition(|r| !catalog.retired.contains(&r.bot));
    for (i, r) in rows.iter_mut().enumerate() {
        r.rank = i as u32 + 1;
    }
    Board { games: games.len(), rows, retired_rows, times, match_ids }
}

/// Retires every bot that has had a fair trial and sits outside the top [`ACTIVE_TOP`]
/// of the standings: arena entrants and roster CPUs.
fn retire_outside_top(app: &AppState, catalog: &Catalog) {
    let rows = board(app, catalog).rows;
    let cut = rows.get(ACTIVE_TOP - 1).map(|r| r.elo);
    for row in &rows {
        if !row.rated || row.rank as usize <= ACTIVE_TOP {
            continue;
        }
        let reason = format!(
            "ranked #{} with Elo {:.0} after {} games; the top {ACTIVE_TOP} cut was {:.0}",
            row.rank,
            row.elo,
            row.games,
            cut.unwrap_or(row.elo)
        );
        let done = if catalog.entrants.iter().any(|e| e.id == row.bot) {
            hexarena_arena::entrants::retire(&app.arena_dir, &row.bot, Some(reason.clone())).map(drop)
        } else {
            hexarena_arena::entrants::retire_other(&app.arena_dir, &row.bot, reason.clone(), now_ms())
        };
        match done {
            Ok(()) => println!("retired {}: {reason}", row.bot),
            Err(err) => eprintln!("couldn't retire {}: {err}", row.bot),
        }
    }
}

fn env<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

#[tokio::main]
async fn main() {
    let dir = std::env::var("HEXARENA_DATA").unwrap_or_else(|_| "data/matches".into());
    let host: IpAddr = env("HEXARENA_HOST").unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let port: u16 = env("HEXARENA_PORT")
        .or_else(|| env("PORT"))
        .unwrap_or(47240);
    let arena_dir = std::env::var("HEXARENA_ARENA").unwrap_or_else(|_| "data/arena".into());
    let mut state = AppState::load(PathBuf::from(&dir), PathBuf::from(&arena_dir));
    state.autoplay = std::env::var("HEXARENA_AUTOPLAY").map_or(true, |v| v != "0");
    state.autoplay_parallel = env("HEXARENA_AUTOPLAY_PARALLEL").unwrap_or(AUTOPLAY_PARALLEL).max(1);
    let app: Shared = Arc::new(state);
    if app.autoplay {
        tokio::spawn(autoplay(app.clone()));
        println!(
            "self-playing arena on: {} 8-player games at a time with entrants from {arena_dir} (HEXARENA_AUTOPLAY=0 turns it off)",
            app.autoplay_parallel
        );
    }
    let mut router = Router::new()
        .route("/api/bots", get(bots))
        .route("/api/bots/{id}/profile", get(profile))
        .route("/api/matches", get(list).post(create))
        .route("/api/matches/{id}", get(one))
        .route("/api/matches/{id}/viewer", get(viewer))
        .route("/api/matches/{id}/replay", get(replay))
        .route("/api/matches/{id}/elo", get(match_elo))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/rules", get(|| async { Json(RulesConfig::default()) }))
        .route("/api/arena", get(arena_info))
        .route("/api/observations", get(list_notes).post(add_note))
        .route("/api/agents", get(agents))
        .route("/api/agents/{agent}/messages", get(thread).post(say))
        .route("/api/agents/{agent}/status", axum::routing::post(set_status))
        .route("/api/agents/{agent}/activity", get(activity))
        .route("/api/health", get(|| async { "ok" }))
        .with_state(app);
    if let Ok(web) = std::env::var("HEXARENA_WEB") {
        let index = PathBuf::from(&web).join("index.html");
        router = router.fallback_service(ServeDir::new(&web).fallback(ServeFile::new(index)));
        println!("serving web app from {web}");
    }
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .expect("bind");
    println!("hexarena server on http://{host}:{port} (data in {dir})");
    axum::serve(listener, router).await.unwrap();
}
