//! What each seat did in a game, boiled down from the replay's events: what it built,
//! when its first Wall went up, how often it attacked and lost hexes. Kept in a small
//! `{id}.habits.json` next to each replay so comparisons never reparse multi-megabyte
//! replays; new games write theirs as they finish and older ones are backfilled.

use hexarena_protocol::{Building, Event};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Habits {
    /// Farms, Villages, Quarries and Walls built.
    pub built: [u32; 4],
    /// Tick of the first Wall, if any.
    pub first_wall: Option<u32>,
    pub demolished: u32,
    /// Hexes taken by attacking, neutral or not.
    pub captures: u32,
    /// Of those, hexes taken from rivals.
    pub rival_captures: u32,
    /// Attacks that bounced.
    pub repelled: u32,
    /// Own hexes rivals took by attacking.
    pub lost: u32,
    /// Hexes won by sealing off a region.
    pub sealed: u32,
    /// Own hexes lost to a rival's seal.
    pub sealed_lost: u32,
}

fn slot(b: Building) -> usize {
    match b {
        Building::Farm => 0,
        Building::Village => 1,
        Building::Quarry => 2,
        Building::Wall => 3,
    }
}

/// One entry per seat; `events[t]` holds what happened on tick `t + 1`.
pub fn from_events(players: usize, events: &[Vec<Event>]) -> Vec<Habits> {
    let mut h = vec![Habits::default(); players];
    for (t, tick) in events.iter().enumerate() {
        for e in tick {
            match e {
                Event::Built { player, building, .. } => {
                    if let Some(s) = h.get_mut(*player as usize) {
                        s.built[slot(*building)] += 1;
                        if *building == Building::Wall && s.first_wall.is_none() {
                            s.first_wall = Some(t as u32 + 1);
                        }
                    }
                }
                Event::Demolished { player, .. } => {
                    if let Some(s) = h.get_mut(*player as usize) {
                        s.demolished += 1;
                    }
                }
                Event::Captured { player, previous_owner, .. } => {
                    if let Some(s) = h.get_mut(*player as usize) {
                        s.captures += 1;
                        s.rival_captures += previous_owner.is_some() as u32;
                    }
                    if let Some(s) = previous_owner.and_then(|p| h.get_mut(p as usize)) {
                        s.lost += 1;
                    }
                }
                Event::Repelled { attacker, .. } => {
                    if let Some(s) = h.get_mut(*attacker as usize) {
                        s.repelled += 1;
                    }
                }
                Event::Surrendered { capturer, from, hexes } => {
                    if let Some(s) = h.get_mut(*capturer as usize) {
                        s.sealed += hexes.len() as u32;
                    }
                    if let Some(s) = h.get_mut(*from as usize) {
                        s.sealed_lost += hexes.len() as u32;
                    }
                }
                _ => {}
            }
        }
    }
    h
}

#[derive(Deserialize)]
struct EventsOnly {
    #[serde(default)]
    events: Vec<Vec<Event>>,
}

pub struct Store {
    dir: PathBuf,
    games: Mutex<HashMap<String, Arc<Vec<Habits>>>>,
}

impl Store {
    /// Loads every habits file already in `dir`.
    pub fn load(dir: &Path) -> Store {
        let mut games = HashMap::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".habits.json") else { continue };
            if let Ok(h) = serde_json::from_slice::<Vec<Habits>>(&std::fs::read(entry.path()).unwrap_or_default()) {
                games.insert(id.to_string(), Arc::new(h));
            }
        }
        Store { dir: dir.to_path_buf(), games: Mutex::new(games) }
    }

    pub fn get(&self, id: &str) -> Option<Arc<Vec<Habits>>> {
        self.games.lock().unwrap().get(id).cloned()
    }

    pub fn has(&self, id: &str) -> bool {
        self.games.lock().unwrap().contains_key(id)
    }

    pub fn save(&self, id: &str, habits: Vec<Habits>) {
        let _ = std::fs::write(self.dir.join(format!("{id}.habits.json")), serde_json::to_vec(&habits).unwrap());
        self.games.lock().unwrap().insert(id.to_string(), Arc::new(habits));
    }

    /// Reads a finished game's replay and records its habits.
    pub fn backfill(&self, id: &str, players: usize) -> Result<(), String> {
        let bytes = std::fs::read(self.dir.join(format!("{id}.replay.json"))).map_err(|e| e.to_string())?;
        let doc: EventsOnly = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        self.save(id, from_events(players, &doc.events));
        Ok(())
    }
}
