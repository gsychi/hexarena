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
use league::Catalog;
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
/// Entrant seats in each self-play game; the rest are the champion and roster CPUs.
const AUTOPLAY_ENTRANTS: usize = 4;
/// Arena entrants stay scheduled while they rank in this many places of the standings.
const ACTIVE_TOP: usize = 20;
/// Games an entrant plays before it can be retired for its ranking.
const TRIAL_GAMES: u32 = 12;

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
    league_dir: PathBuf,
    arena_dir: PathBuf,
    matches: Mutex<BTreeMap<String, Summary>>,
    league_games: Mutex<league::LeagueGames>,
    next_id: Mutex<u64>,
    workers: Semaphore,
    autoplay: bool,
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

    fn catalog(&self) -> Catalog {
        Catalog::load(&self.league_dir, &self.arena_dir)
    }

    fn running(&self) -> usize {
        self.matches.lock().unwrap().values().filter(|m| m.status == Status::Running).count()
    }

    fn load(dir: PathBuf, league_dir: PathBuf, arena_dir: PathBuf) -> AppState {
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
            arena_dir,
            matches: Mutex::new(matches),
            league_games: Mutex::new(league::LeagueGames::default()),
            next_id: Mutex::new(next),
            workers: Semaphore::new(workers),
            autoplay: false,
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
    Json(serde_json::json!(app.catalog().bots()))
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

/// Seats for the next self-play game: the active entrants with the fewest arena games
/// (up to [`AUTOPLAY_ENTRANTS`]), the league champion, then bots drawn from the rest of
/// the active field (roster CPUs and league generations that aren't retired), in seed order.
fn autoplay_lineup(app: &AppState, catalog: &Catalog, seed: u64) -> Vec<Style> {
    let mut played: BTreeMap<String, usize> = BTreeMap::new();
    for m in app.matches.lock().unwrap().values() {
        if m.status != Status::Failed {
            for seat in &m.seats {
                *played.entry(seat.bot.clone()).or_default() += 1;
            }
        }
    }
    let mut entrants: Vec<&hexarena_arena::entrants::Entrant> = catalog.active_entrants().collect();
    entrants.sort_by_key(|e| (played.get(&e.id).copied().unwrap_or(0), e.created_at));
    let mut seats: Vec<Style> = entrants.iter().take(AUTOPLAY_ENTRANTS).map(|e| e.style()).collect();
    if let Some(champion) = catalog.resolve("champion") {
        seats.push(champion);
    }
    let mut rng = seed | 1;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let field = catalog.active_field();
    let mut pool: Vec<Style> = vec![];
    while seats.len() < MAX_PLAYERS && !field.is_empty() {
        if pool.is_empty() {
            pool = field.iter().filter(|s| seats.iter().all(|t| t.id != s.id)).cloned().collect();
            if pool.is_empty() {
                pool = field.clone();
            }
        }
        let pick = (next() % pool.len() as u64) as usize;
        seats.push(pool.swap_remove(pick));
    }
    for i in (1..seats.len()).rev() {
        seats.swap(i, (next() % (i as u64 + 1)) as usize);
    }
    seats
}

/// The self-playing arena: whenever no match is running, start an 8-player game.
async fn autoplay(app: Shared) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if app.running() > 0 {
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
            })
        })
        .collect();
    Json(serde_json::json!({
        "autoplay": app.autoplay,
        "top": ACTIVE_TOP,
        "trial_games": TRIAL_GAMES,
        "entrants": entrants,
        "retired": catalog.retired_reasons(),
    }))
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

/// Which finished games the standings count.
#[derive(Deserialize, Clone, Copy, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Source {
    #[default]
    All,
    Arena,
    League,
}

#[derive(Deserialize)]
struct BoardQuery {
    #[serde(default)]
    source: Source,
}

/// Standings only count games played under the current rules, since the rules and
/// the CPUs change together. Arena games are the ones created here; league games are
/// the league's 8-player lobbies, where generations and roster CPUs are rated and
/// one-off challengers are left out.
async fn leaderboard(State(app): State<Shared>, Query(q): Query<BoardQuery>) -> Json<serde_json::Value> {
    let catalog = app.catalog();
    let board = board(&app, &catalog, q.source);
    Json(serde_json::json!({
        "games": board.games,
        "arena_games": board.arena_games,
        "league_games": board.league_games,
        "rules_version": RulesConfig::default().version,
        "start_elo": START_ELO,
        "k": K,
        "top": ACTIVE_TOP,
        "retired": catalog.retired_reasons(),
        "bots": board.rows,
        "retired_bots": board.retired_rows,
    }))
}

struct Board {
    games: usize,
    arena_games: usize,
    league_games: usize,
    /// Active bots, ranked among themselves.
    rows: Vec<standings::BotStanding>,
    /// Retired bots, kept out of the standings. Ratings still count every game.
    retired_rows: Vec<standings::BotStanding>,
}

fn board(app: &AppState, catalog: &Catalog, source: Source) -> Board {
    let rules = RulesConfig::default().version;
    let current = catalog.league.as_ref();
    let matches = app.matches.lock().unwrap();
    let mut done: Vec<&Summary> = matches
        .values()
        .filter(|m| m.status == Status::Done && m.rules_version == rules)
        .collect();
    done.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
    let mut cache = app.league_games.lock().unwrap();
    cache.refresh(&app.league_dir);
    let league_current = current.is_some_and(|l| l.rules_version == rules);

    let mut games: Vec<(u64, Vec<SeatResult>)> = vec![];
    if source != Source::League {
        games.extend(done.iter().map(|m| {
            let seats = m
                .standings
                .iter()
                .map(|s| SeatResult {
                    bot: &s.bot,
                    place: (s.place_start + s.place_end) as f64 / 2.0,
                    normalized: s.normalized,
                    won: s.place_start == 1 && s.place_end == 1,
                    players: m.standings.len(),
                })
                .collect();
            (m.created_at, seats)
        }));
    }
    let arena_games = if source == Source::League { 0 } else { done.len() };
    let mut league_games = 0;
    if source != Source::Arena && league_current {
        let names = &cache.names;
        league_games = cache.games.len();
        games.extend(cache.games.iter().map(|g| {
            let n = g.players as f64;
            let seats = g
                .seats
                .iter()
                .map(|&(i, avp)| {
                    let place = 1.0 + (avp - 1.0) * (n - 1.0) / 7.0;
                    SeatResult {
                        bot: &names[i],
                        place,
                        normalized: (n - place) / (n - 1.0),
                        won: place == 1.0,
                        players: g.players,
                    }
                })
                .collect();
            (g.at, seats)
        }));
    }
    games.sort_by_key(|(at, _)| *at);
    let games: Vec<Vec<SeatResult>> = games.into_iter().map(|(_, g)| g).collect();
    let (mut rows, retired_rows): (Vec<_>, Vec<_>) = standings(&games, |id| catalog.display_name(id))
        .into_iter()
        .partition(|r| !catalog.retired.contains(&r.bot));
    for (i, r) in rows.iter_mut().enumerate() {
        r.rank = i as u32 + 1;
    }
    Board { games: games.len(), arena_games, league_games, rows, retired_rows }
}

/// Retires every bot that has had a fair trial and sits outside the top [`ACTIVE_TOP`]
/// of the overall standings: arena entrants, roster CPUs and league generations. The
/// current league champion stays, since it is the bar everything else is measured by.
fn retire_outside_top(app: &AppState, catalog: &Catalog) {
    let rows = board(app, catalog, Source::All).rows;
    let cut = rows.get(ACTIVE_TOP - 1).map(|r| r.elo);
    let champion = catalog.league.as_ref().map(|l| l.champion().id.clone());
    for row in &rows {
        if row.games < TRIAL_GAMES || row.rank as usize <= ACTIVE_TOP {
            continue;
        }
        if Some(&row.bot) == champion.as_ref() {
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
    let league_dir = std::env::var("HEXARENA_LEAGUE").unwrap_or_else(|_| "data/league".into());
    let arena_dir = std::env::var("HEXARENA_ARENA").unwrap_or_else(|_| "data/arena".into());
    let mut state = AppState::load(PathBuf::from(&dir), PathBuf::from(league_dir), PathBuf::from(&arena_dir));
    state.autoplay = std::env::var("HEXARENA_AUTOPLAY").map_or(true, |v| v != "0");
    let app: Shared = Arc::new(state);
    if app.autoplay {
        tokio::spawn(autoplay(app.clone()));
        println!("self-playing arena on: 8-player games with entrants from {arena_dir} (HEXARENA_AUTOPLAY=0 turns it off)");
    }
    let mut router = Router::new()
        .route("/api/bots", get(bots))
        .route("/api/matches", get(list).post(create))
        .route("/api/matches/{id}", get(one))
        .route("/api/matches/{id}/viewer", get(viewer))
        .route("/api/matches/{id}/replay", get(replay))
        .route("/api/leaderboard", get(leaderboard))
        .route("/api/league", get(league_snapshot))
        .route("/api/arena", get(arena_info))
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
