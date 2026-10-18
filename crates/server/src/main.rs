//! Hexarena game server: creates CPU matches on the authoritative engine, stores
//! replays on disk, and serves match summaries, viewer documents and a leaderboard.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use hexarena_arena::{play, seat_names, viewer_json};
use hexarena_protocol::{PlayerId, RulesConfig};
use hexarena_sdk::Style;
use serde::{Deserialize, Serialize};
use standings::{standings, SeatResult, K, START_ELO};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use tower_http::services::{ServeDir, ServeFile};

mod league;
mod standings;

const MAX_PLAYERS: usize = 8;
const MAX_BATCH: u32 = 20;
/// Refuse new games while this many are still queued or running.
const MAX_RUNNING: usize = 100;

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
    /// Everyone else was eliminated (v0 rules only).
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
}

#[derive(Deserialize)]
struct CreateMatch {
    bots: Vec<String>,
    seed: Option<u64>,
    count: Option<u32>,
}

struct AppState {
    dir: PathBuf,
    league_dir: PathBuf,
    matches: Mutex<BTreeMap<String, Summary>>,
    next_id: Mutex<u64>,
    workers: Semaphore,
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

    fn load(dir: PathBuf, league_dir: PathBuf) -> AppState {
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
            league_dir,
            matches: Mutex::new(matches),
            next_id: Mutex::new(next),
            workers: Semaphore::new(workers),
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
        let ending = if played.state.conqueror().is_some() {
            if played.state.rules().end_on_total_conquest {
                Ending::Conquest
            } else {
                Ending::LastStanding
            }
        } else {
            Ending::Cutoff
        };
        Ok((played.result(), ending))
    });
    match outcome {
        Ok((result, ending)) => {
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
    Json(serde_json::json!(league::bots(league::load(&app.league_dir).as_ref())))
}

async fn league_snapshot(State(app): State<Shared>) -> Response {
    match league::load(&app.league_dir) {
        Some(l) => Json(league::snapshot(l)).into_response(),
        None => err(
            StatusCode::NOT_FOUND,
            "no league yet: start one with `hexarena league run`",
        ),
    }
}

async fn list(State(app): State<Shared>) -> Json<Vec<Summary>> {
    let mut v: Vec<Summary> = app.matches.lock().unwrap().values().cloned().collect();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
    Json(v)
}

async fn create(State(app): State<Shared>, Json(req): Json<CreateMatch>) -> Response {
    if req.bots.len() < 2 || req.bots.len() > MAX_PLAYERS {
        return err(
            StatusCode::BAD_REQUEST,
            format!("pick 2 to {MAX_PLAYERS} CPU players"),
        );
    }
    let current = league::load(&app.league_dir);
    let styles: Result<Vec<Style>, String> = req
        .bots
        .iter()
        .map(|id| league::resolve(id, current.as_ref()).ok_or_else(|| format!("unknown bot '{id}'")))
        .collect();
    let styles = match styles {
        Ok(s) => s,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let count = req.count.unwrap_or(1).clamp(1, MAX_BATCH);
    let running = app
        .matches
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.status == Status::Running)
        .count();
    if running + count as usize > MAX_RUNNING {
        return err(
            StatusCode::TOO_MANY_REQUESTS,
            "the arena is busy, try again once some games finish",
        );
    }
    let max_ticks = RulesConfig::default().max_ticks;
    let base_seed = req
        .seed
        .unwrap_or_else(|| now_ms().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 16);
    let names = seat_names(&styles);
    let mut created = vec![];
    for i in 0..count {
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
            seed: base_seed.wrapping_add(i as u64),
            max_ticks,
            seats: styles
                .iter()
                .zip(&names)
                .map(|(s, n)| Seat {
                    bot: s.id.clone(),
                    name: n.clone(),
                })
                .collect(),
            final_tick: None,
            winner: None,
            standings: vec![],
        };
        app.save(&summary);
        created.push(summary.clone());
        let (app, styles) = (app.clone(), styles.clone());
        tokio::spawn(async move {
            let _permit = app.workers.acquire().await.unwrap();
            let worker = app.clone();
            let done = tokio::task::spawn_blocking(move || run_match(&worker, summary, styles))
                .await
                .expect("match task panicked");
            app.save(&done);
        });
    }
    (StatusCode::CREATED, Json(created)).into_response()
}

async fn one(State(app): State<Shared>, Path(id): Path<String>) -> Response {
    match app.matches.lock().unwrap().get(&id) {
        Some(s) => Json(s.clone()).into_response(),
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

/// Standings only count games played under the current rules, since the rules and
/// the CPUs change together.
async fn leaderboard(State(app): State<Shared>) -> Json<serde_json::Value> {
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
                })
                .collect()
        })
        .collect();
    let current = league::load(&app.league_dir);
    let rows = standings(&games, |id| league::display_name(id, current.as_ref()));
    Json(serde_json::json!({
        "games": done.len(),
        "rules_version": rules,
        "start_elo": START_ELO,
        "k": K,
        "bots": rows,
    }))
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
    let league_dir = std::env::var("HEXARENA_LEAGUE").unwrap_or_else(|_| "data/league".into());
    let app: Shared = Arc::new(AppState::load(PathBuf::from(&dir), PathBuf::from(league_dir)));
    let mut router = Router::new()
        .route("/api/bots", get(bots))
        .route("/api/matches", get(list).post(create))
        .route("/api/matches/{id}", get(one))
        .route("/api/matches/{id}/viewer", get(viewer))
        .route("/api/matches/{id}/replay", get(replay))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/league", get(league_snapshot))
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
