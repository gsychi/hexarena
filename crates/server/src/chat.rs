//! Messages between the people watching the arena and the agents writing its bots: one
//! thread per agent, stored one JSON object per line in `messages.jsonl` in the arena
//! dir. Agents read their thread at the start of every round and reply in it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

const MAX_TEXT: usize = 2000;
const MAX_STATUS: usize = 400;
/// The sender name for whoever is watching in the browser.
pub const VISITOR: &str = "you";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub n: u64,
    pub at: u64,
    /// The thread: the agent it is to or from, e.g. `agent:sol`.
    pub agent: String,
    /// [`VISITOR`] or the agent itself.
    pub from: String,
    pub text: String,
}

#[derive(Deserialize)]
pub struct NewMessage {
    pub from: String,
    pub text: String,
}

/// What an agent says it is working on right now, set by the agent.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub at: u64,
    pub text: String,
}

#[derive(Deserialize)]
pub struct NewStatus {
    pub text: String,
}

pub struct Chat {
    lock: Mutex<()>,
}

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("messages.jsonl")
}

fn status_path(dir: &Path) -> std::path::PathBuf {
    dir.join("status.json")
}

impl Chat {
    pub fn new() -> Chat {
        Chat { lock: Mutex::new(()) }
    }

    /// Oldest first.
    pub fn all(&self, dir: &Path) -> Vec<Message> {
        let _g = self.lock.lock().unwrap();
        std::fs::read_to_string(path(dir))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn statuses(&self, dir: &Path) -> BTreeMap<String, Status> {
        let _g = self.lock.lock().unwrap();
        std::fs::read(status_path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn set_status(&self, dir: &Path, agent: &str, new: NewStatus, at: u64) -> Result<Status, String> {
        let text = new.text.trim().to_string();
        if text.is_empty() || text.len() > MAX_STATUS {
            return Err(format!("text must be 1-{MAX_STATUS} characters"));
        }
        let mut all = self.statuses(dir);
        let status = Status { at, text };
        all.insert(agent.to_string(), status.clone());
        let _g = self.lock.lock().unwrap();
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = status_path(dir).with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&all).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, status_path(dir)).map_err(|e| e.to_string())?;
        Ok(status)
    }

    pub fn thread(&self, dir: &Path, agent: &str) -> Vec<Message> {
        self.all(dir).into_iter().filter(|m| m.agent == agent).collect()
    }

    pub fn add(&self, dir: &Path, agent: &str, new: NewMessage, at: u64) -> Result<Message, String> {
        let from = new.from.trim().to_string();
        let text = new.text.trim().to_string();
        if from != VISITOR && from != agent {
            return Err(format!("from must be \"{VISITOR}\" or \"{agent}\""));
        }
        if text.is_empty() || text.len() > MAX_TEXT {
            return Err(format!("text must be 1-{MAX_TEXT} characters"));
        }
        let n = self.all(dir).last().map_or(1, |m| m.n + 1);
        let msg = Message { n, at, agent: agent.to_string(), from, text };
        let _g = self.lock.lock().unwrap();
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path(dir))
            .map_err(|e| e.to_string())?;
        writeln!(f, "{}", serde_json::to_string(&msg).unwrap()).map_err(|e| e.to_string())?;
        Ok(msg)
    }
}
