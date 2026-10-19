//! Live games where a person holds one seat against CPU bots. Each game ticks on its own
//! thread at the speed the player picks; the person's orders wait in a queue and the
//! next one runs each tick, exactly like a bot's one action per tick.

use hexarena_arena::{seat_names, viewer_json};
use hexarena_engine::{build_replay, europe, State};
use hexarena_protocol::*;
use hexarena_sdk::{brains, style, Bot, Style};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SPEEDS: [u32; 6] = [1, 2, 5, 10, 20, 40];
pub const DEFAULT_SPEED: u32 = 5;
/// Orders a player may have waiting at once.
pub const QUEUE_LIMIT: usize = 40;
/// Live games at once; finished and abandoned ones don't count.
pub const MAX_LIVE: usize = 4;
/// A game nobody has looked at for this long pauses itself...
const IDLE_PAUSE: Duration = Duration::from_secs(45);
/// ...and after this long it is abandoned and saved as it stands.
const IDLE_ABANDON: Duration = Duration::from_secs(30 * 60);
/// Roster CPUs that can play the person's seat on ticks with no order waiting.
pub const ASSISTS: [&str; 4] = ["expander", "homesteader", "raider", "warden"];
pub const DEFAULT_ASSIST: &str = "expander";
/// Ticks of the player's visible events kept for the client to animate and log.
const EVENT_HISTORY: usize = 600;

/// What the player asked for. A move names a share of the hex's spare troops rather
/// than a count, so it still makes sense when it runs a few ticks later.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Order {
    Move { from: Hex, to: Hex, share_pct: u32 },
    Gather { target: Hex },
    Build { at: Hex, building: Building },
    Demolish { at: Hex },
}

impl Order {
    fn action(&self, st: &State) -> Action {
        match *self {
            Order::Move { from, to, share_pct } => {
                let spare = st.cell(from).map_or(0, |c| c.troops.saturating_sub(1));
                let troops = if spare == 0 { 0 } else { (spare * share_pct.clamp(1, 100)).div_ceil(100) };
                Action::Move { from, to, troops }
            }
            Order::Gather { target } => Action::Gather { target },
            Order::Build { at, building } => Action::Build { at, building },
            Order::Demolish { at } => Action::Demolish { at },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Seat {
    pub bot: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Playing,
    Won,
    Lost,
    /// The tick cutoff came first; `result` holds the places.
    Cutoff,
    Abandoned,
}

pub struct Live {
    pub id: String,
    pub created_at: u64,
    pub you: PlayerId,
    pub seats: Vec<Seat>,
    pub state: State,
    pub queue: VecDeque<Order>,
    pub speed: u32,
    pub paused: bool,
    pub phase: Phase,
    /// Roster CPU that plays the person's seat whenever the queue is empty.
    pub assist: Option<String>,
    commands: Vec<TickCommands>,
    events: Vec<Vec<Event>>,
    /// The player's visible events by tick, newest last.
    seen: VecDeque<(Tick, Vec<Event>)>,
    polled: Instant,
}

pub type Game = Arc<Mutex<Live>>;

pub struct Games {
    dir: PathBuf,
    games: Mutex<BTreeMap<String, Game>>,
    next: Mutex<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// The hand-written roster CPUs.
    Easy,
    /// Arena bots from the middle of the standings.
    Medium,
    /// The top of the standings.
    Hard,
}

#[derive(Deserialize)]
pub struct NewGame {
    #[serde(default)]
    pub map: Option<MapPreset>,
    pub level: Level,
    /// CPU opponents, 1 to 7.
    pub opponents: usize,
    #[serde(default)]
    pub speed: Option<u32>,
}

#[derive(Deserialize)]
pub struct NewOrders {
    pub orders: Vec<Order>,
    #[serde(default)]
    pub clear: bool,
    /// Drop the newest waiting order before adding these.
    #[serde(default)]
    pub undo: bool,
}

#[derive(Deserialize)]
pub struct Control {
    pub speed: Option<u32>,
    pub paused: Option<bool>,
    /// A roster CPU id from [`ASSISTS`], or `off`.
    pub assist: Option<String>,
    #[serde(default)]
    pub resign: bool,
}

#[derive(Deserialize)]
pub struct Since {
    #[serde(default)]
    pub since: Tick,
}

impl Games {
    pub fn new(dir: PathBuf) -> Games {
        std::fs::create_dir_all(&dir).expect("create play dir");
        let next = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.strip_prefix('p')?.split('.').next()?.parse::<u64>().ok()
            })
            .max()
            .unwrap_or(0)
            + 1;
        Games { dir, games: Mutex::new(BTreeMap::new()), next: Mutex::new(next) }
    }

    pub fn get(&self, id: &str) -> Option<Game> {
        self.games.lock().unwrap().get(id).cloned()
    }

    pub fn viewer_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.viewer.json"))
    }

    pub fn list(&self) -> Vec<Game> {
        self.games.lock().unwrap().values().cloned().collect()
    }

    fn live(&self) -> usize {
        self.games.lock().unwrap().values().filter(|g| g.lock().unwrap().phase == Phase::Playing).count()
    }

    /// Starts a game with the person in a random seat and `opponents` in the others.
    pub fn start(&self, map: Option<MapPreset>, opponents: Vec<Style>, speed: u32, seed: u64, now: u64) -> Result<Game, String> {
        if self.live() >= MAX_LIVE {
            return Err(format!("{MAX_LIVE} games are already going; finish or resign one first"));
        }
        let players = opponents.len() + 1;
        let mut rules = RulesConfig::default();
        rules.map = map;
        let settings = MatchSettings {
            players: players as u16,
            terrain_seed: seed,
            spawn_seed: seed.wrapping_mul(31).wrapping_add(7),
            match_seed: seed.wrapping_mul(13).wrapping_add(5),
            rules,
        };
        let you = (seed % players as u64) as PlayerId;
        let mut styles: Vec<Option<Style>> = opponents.into_iter().map(Some).collect();
        styles.insert(you as usize, None);
        let bot_names = seat_names(&styles.iter().flatten().cloned().collect::<Vec<_>>());
        let mut bot_names = bot_names.into_iter();
        let seats: Vec<Seat> = styles
            .iter()
            .map(|s| match s {
                Some(s) => Seat { bot: s.id.clone(), name: bot_names.next().unwrap() },
                None => Seat { bot: "you".into(), name: "You".into() },
            })
            .collect();
        let state = State::new(settings, seats.iter().map(|s| s.name.clone()).collect())?;
        let id = {
            let mut n = self.next.lock().unwrap();
            let id = format!("p{:05}", *n);
            *n += 1;
            id
        };
        let game: Game = Arc::new(Mutex::new(Live {
            id: id.clone(),
            created_at: now,
            you,
            seats,
            state,
            queue: VecDeque::new(),
            speed: if SPEEDS.contains(&speed) { speed } else { DEFAULT_SPEED },
            paused: false,
            phase: Phase::Playing,
            assist: Some(DEFAULT_ASSIST.into()),
            commands: vec![],
            events: vec![],
            seen: VecDeque::new(),
            polled: Instant::now(),
        }));
        self.games.lock().unwrap().insert(id, game.clone());
        let (thread_game, dir) = (game.clone(), self.dir.clone());
        std::thread::spawn(move || run(thread_game, styles, &dir));
        Ok(game)
    }
}

fn run(game: Game, styles: Vec<Option<Style>>, dir: &Path) {
    let built: Result<Vec<Option<Box<dyn Bot>>>, String> =
        styles.into_iter().map(|s| s.map(brains::build).transpose()).collect();
    let mut bots = match built {
        Ok(b) => b,
        Err(e) => {
            eprintln!("live game bots failed to start: {e}");
            game.lock().unwrap().phase = Phase::Abandoned;
            return;
        }
    };
    let mut helper: Option<(String, Box<dyn Bot>)> = None;
    loop {
        let (speed, paused, idle, assist) = {
            let g = game.lock().unwrap();
            if g.phase != Phase::Playing {
                break;
            }
            (g.speed, g.paused, g.polled.elapsed(), g.assist.clone())
        };
        if helper.as_ref().map(|h| &h.0) != assist.as_ref() {
            helper = assist.and_then(|id| Some((id.clone(), brains::build(style(&id)?).ok()?)));
        }
        if idle > IDLE_ABANDON {
            game.lock().unwrap().phase = Phase::Abandoned;
            break;
        }
        if paused || idle > IDLE_PAUSE {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let started = Instant::now();
        let views: Vec<(PlayerId, View)> = {
            let g = game.lock().unwrap();
            (0..g.seats.len() as PlayerId)
                .filter(|&p| p != g.you && !g.state.eliminated[p as usize])
                .map(|p| (p, g.state.view(p)))
                .collect()
        };
        let mut cmds: TickCommands =
            views.iter().map(|(p, v)| (*p, bots[*p as usize].as_mut().unwrap().act(v))).collect();
        let mut g = game.lock().unwrap();
        let you = g.you;
        if !g.state.eliminated[you as usize] {
            let cmd = match g.queue.pop_front() {
                Some(o) => PlayerCommand::action(o.action(&g.state)),
                None => match helper.as_mut() {
                    Some((_, bot)) => bot.act(&g.state.view(you)),
                    None => PlayerCommand::default(),
                },
            };
            cmds.push((you, cmd));
        }
        cmds.sort_by_key(|c| c.0);
        g.state.step(&cmds);
        let events = g.state.last_events.clone();
        g.commands.push(cmds);
        g.events.push(events);
        let tick = g.state.tick;
        let visible = g.state.view(you).visible_events;
        if !visible.is_empty() {
            g.seen.push_back((tick, visible));
        }
        while g.seen.front().is_some_and(|(t, _)| t + (EVENT_HISTORY as Tick) < tick) {
            g.seen.pop_front();
        }
        if g.state.is_over() || tick >= g.state.rules().max_ticks || g.state.eliminated[you as usize] {
            g.phase = finish_phase(&g);
        }
        if g.phase != Phase::Playing {
            break;
        }
        drop(g);
        let wait = Duration::from_secs_f64(1.0 / speed as f64);
        if let Some(rest) = wait.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    let g = game.lock().unwrap();
    save(&g, dir);
}

fn finish_phase(g: &Live) -> Phase {
    match g.state.conqueror() {
        Some(p) if p == g.you => Phase::Won,
        Some(_) => Phase::Lost,
        None if g.state.eliminated[g.you as usize] => Phase::Lost,
        None => Phase::Cutoff,
    }
}

fn save(g: &Live, dir: &Path) {
    let replay = build_replay(&g.state, g.commands.clone(), g.events.clone());
    let _ = std::fs::write(dir.join(format!("{}.replay.json", g.id)), serde_json::to_vec(&replay).unwrap());
    match viewer_json(&replay) {
        Ok(v) => {
            let _ = std::fs::write(dir.join(format!("{}.viewer.json", g.id)), serde_json::to_vec(&v).unwrap());
        }
        Err(e) => eprintln!("couldn't build the viewer for {}: {e}", g.id),
    }
}

impl Live {
    /// The board, terrain and seats: everything that never changes during the game.
    pub fn setup(&self) -> serde_json::Value {
        let st = &self.state;
        let places: Vec<serde_json::Value> = match st.rules().map {
            Some(MapPreset::Europe) => europe::places(st.board.radius)
                .into_iter()
                .filter_map(|(name, h)| st.board.index(h).map(|i| serde_json::json!({ "name": name, "idx": i })))
                .collect(),
            None => vec![],
        };
        serde_json::json!({
            "id": self.id,
            "radius": st.board.radius,
            "fog": st.rules().fog_radius,
            "rules": st.rules(),
            "terrain": st.terrain,
            "names": st.names,
            "seats": self.seats,
            "you": self.you,
            "map": st.rules().map,
            "places": places,
            "speeds": SPEEDS,
            "assists": ASSISTS.iter().filter_map(|id| style(id)).map(|s| serde_json::json!({ "id": s.id, "name": s.name, "blurb": s.blurb })).collect::<Vec<_>>(),
        })
    }

    /// What the player can see now. `since` skips events the client already has.
    pub fn snapshot(&mut self, since: Tick) -> serde_json::Value {
        self.polled = Instant::now();
        let st = &self.state;
        let view = st.view(self.you);
        let idx = |h: Hex| st.board.index(h).map_or(-1, |i| i as i64);
        let mut cells: Vec<i64> = Vec::with_capacity(view.visible_hexes.len() * 9);
        let mut visible: Vec<i64> = Vec::with_capacity(view.visible_hexes.len());
        for c in &view.visible_hexes {
            let i = idx(c.hex);
            visible.push(i);
            let (kind, remaining, links, cooldown) = match &c.building {
                Some(b) => (b.kind as i64 + 1, b.production_remaining as i64, b.links.clone(), b.link_cooldown as i64),
                None => (0, 0, vec![], 0),
            };
            let link = |k: usize| links.get(k).map_or(-1, |&h| idx(h));
            cells.extend([
                i,
                c.owner.map_or(-1, |o| o as i64),
                c.troops as i64,
                kind,
                remaining,
                link(0),
                link(1),
                link(2),
                cooldown,
            ]);
        }
        let events: Vec<&(Tick, Vec<Event>)> = self.seen.iter().filter(|(t, _)| *t > since).collect();
        let result = (self.phase != Phase::Playing).then(|| st.result.clone().unwrap_or_else(|| st.final_result()));
        serde_json::json!({
            "tick": st.tick,
            "max_ticks": st.rules().max_ticks,
            "speed": self.speed,
            "paused": self.paused,
            "assist": self.assist,
            "phase": self.phase,
            "alive": st.eliminated.iter().map(|e| !e).collect::<Vec<_>>(),
            "land": st.hex_counts(),
            "cells": cells,
            "visible": visible,
            "queue": self.queue,
            "events": events,
            "result": result,
            "replay": (self.phase != Phase::Playing).then(|| self.id.clone()),
        })
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "created_at": self.created_at,
            "tick": self.state.tick,
            "phase": self.phase,
            "map": self.state.rules().map,
            "seats": self.seats,
            "you": self.you,
        })
    }

    pub fn add_orders(&mut self, req: NewOrders) -> Result<(), String> {
        let NewOrders { orders, clear, undo } = req;
        if self.phase != Phase::Playing {
            return Err("the game is over".into());
        }
        if clear {
            self.queue.clear();
        }
        if undo {
            self.queue.pop_back();
        }
        if self.queue.len() + orders.len() > QUEUE_LIMIT {
            return Err(format!("at most {QUEUE_LIMIT} orders can wait at once"));
        }
        self.queue.extend(orders);
        Ok(())
    }

    pub fn control(&mut self, c: Control) -> Result<(), String> {
        let Control { speed, paused, assist, resign } = c;
        if let Some(a) = assist {
            match a.as_str() {
                "" | "off" => self.assist = None,
                id if ASSISTS.contains(&id) => self.assist = Some(id.into()),
                _ => return Err(format!("assist must be off or one of {ASSISTS:?}")),
            }
        }
        if let Some(s) = speed {
            if !SPEEDS.contains(&s) {
                return Err(format!("speed must be one of {SPEEDS:?} ticks a second"));
            }
            self.speed = s;
        }
        if let Some(p) = paused {
            self.paused = p;
        }
        if resign && self.phase == Phase::Playing {
            self.phase = Phase::Lost;
        }
        Ok(())
    }
}
