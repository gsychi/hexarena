//! The lab notebook: observations people and agents write down about games and bots,
//! so every bot designer starts from what has already been learned.
//! Stored one JSON object per line in `observations.jsonl` in the arena dir.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

const MAX_TEXT: usize = 4000;
const MAX_LIST: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Something seen in games or replays.
    #[default]
    Observation,
    /// A conclusion backed by a result, e.g. a gate.
    Lesson,
    /// Something worth trying next.
    Idea,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub n: u64,
    pub at: u64,
    pub author: String,
    #[serde(default)]
    pub kind: Kind,
    pub text: String,
    /// Bot ids it is about (`ridgeline`, `g10`, `thumper-v2`).
    #[serde(default)]
    pub bots: Vec<String>,
    /// Match ids it cites (`m01012`).
    #[serde(default)]
    pub matches: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize)]
pub struct NewNote {
    pub author: String,
    #[serde(default)]
    pub kind: Kind,
    pub text: String,
    #[serde(default)]
    pub bots: Vec<String>,
    #[serde(default)]
    pub matches: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize, Default)]
pub struct Filter {
    pub author: Option<String>,
    pub kind: Option<Kind>,
    pub bot: Option<String>,
    pub tag: Option<String>,
    /// Case-insensitive text search.
    pub q: Option<String>,
    pub limit: Option<usize>,
}

pub struct Notebook {
    lock: Mutex<()>,
}

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("observations.jsonl")
}

fn short_list(v: Vec<String>) -> Vec<String> {
    v.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s.len() <= 60).take(MAX_LIST).collect()
}

impl Notebook {
    pub fn new() -> Notebook {
        Notebook { lock: Mutex::new(()) }
    }

    pub fn all(&self, dir: &Path) -> Vec<Note> {
        let _g = self.lock.lock().unwrap();
        std::fs::read_to_string(path(dir))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// Newest first.
    pub fn list(&self, dir: &Path, f: &Filter) -> Vec<Note> {
        let q = f.q.as_ref().map(|q| q.to_lowercase());
        let mut v: Vec<Note> = self
            .all(dir)
            .into_iter()
            .filter(|n| f.author.as_ref().is_none_or(|a| &n.author == a))
            .filter(|n| f.kind.is_none_or(|k| n.kind == k))
            .filter(|n| f.bot.as_ref().is_none_or(|b| n.bots.contains(b)))
            .filter(|n| f.tag.as_ref().is_none_or(|t| n.tags.contains(t)))
            .filter(|n| q.as_ref().is_none_or(|q| n.text.to_lowercase().contains(q)))
            .collect();
        v.reverse();
        v.truncate(f.limit.unwrap_or(500));
        v
    }

    pub fn add(&self, dir: &Path, new: NewNote, at: u64) -> Result<Note, String> {
        let author = new.author.trim().to_string();
        let text = new.text.trim().to_string();
        if author.is_empty() || author.len() > 60 {
            return Err("author must be 1-60 characters, e.g. agent:bunny-opus".into());
        }
        if text.is_empty() || text.len() > MAX_TEXT {
            return Err(format!("text must be 1-{MAX_TEXT} characters"));
        }
        let n = self.all(dir).last().map_or(1, |l| l.n + 1);
        let note = Note {
            n,
            at,
            author,
            kind: new.kind,
            text,
            bots: short_list(new.bots),
            matches: short_list(new.matches),
            tags: short_list(new.tags).into_iter().map(|t| t.to_lowercase()).collect(),
        };
        let _g = self.lock.lock().unwrap();
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path(dir))
            .map_err(|e| e.to_string())?;
        writeln!(f, "{}", serde_json::to_string(&note).unwrap()).map_err(|e| e.to_string())?;
        Ok(note)
    }
}
