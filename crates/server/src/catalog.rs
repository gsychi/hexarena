//! Every bot a seat can name: the hand-written roster CPUs and the arena versions that
//! people and agents publish.

use hexarena_arena::entrants::{author_models, load_all, retired_others, with_model, Entrant, Retirement};
use hexarena_sdk::{roster, style, Style};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub struct Catalog {
    pub entrants: Vec<Entrant>,
    /// Retired roster CPUs.
    pub others_retired: BTreeMap<String, Retirement>,
    /// Every retired id, entrants included.
    pub retired: BTreeSet<String>,
}

impl Catalog {
    pub fn load(arena_dir: &Path) -> Catalog {
        let models = author_models(arena_dir);
        let mut entrants = load_all(arena_dir);
        for e in &mut entrants {
            e.name = with_model(&e.name, models.get(&e.author).map(String::as_str));
        }
        let others_retired = retired_others(arena_dir);
        let mut retired: BTreeSet<String> = others_retired.keys().cloned().collect();
        retired.extend(entrants.iter().filter(|e| e.retired).map(|e| e.id.clone()));
        Catalog { entrants, others_retired, retired }
    }

    /// Why each retired bot was retired, by id.
    pub fn retired_reasons(&self) -> BTreeMap<String, String> {
        let mut out: BTreeMap<String, String> =
            self.others_retired.iter().map(|(id, r)| (id.clone(), r.reason.clone())).collect();
        for e in self.entrants.iter().filter(|e| e.retired) {
            out.insert(e.id.clone(), e.retired_reason.clone().unwrap_or_else(|| "retired".into()));
        }
        out
    }

    /// Roster CPUs that still play in the arena.
    pub fn active_field(&self) -> Vec<Style> {
        roster().into_iter().filter(|s| !self.retired.contains(&s.id)).collect()
    }

    /// A roster style or an arena entrant (retired ones too, so old games keep their names).
    pub fn resolve(&self, id: &str) -> Option<Style> {
        style(id).or_else(|| self.entrants.iter().find(|e| e.id == id).map(Entrant::style))
    }

    pub fn knows(&self, id: &str) -> bool {
        style(id).is_some() || self.entrants.iter().any(|e| e.id == id)
    }

    pub fn display_name(&self, id: &str) -> String {
        self.resolve(id).map(|s| s.name).unwrap_or_else(|| id.to_string())
    }

    pub fn active_entrants(&self) -> impl Iterator<Item = &Entrant> {
        self.entrants.iter().filter(|e| !e.retired)
    }

    /// Every playable bot: roster CPUs, then every arena version (newest first). Retired
    /// bots are still playable and carry `retired` with the reason. `parents` are the
    /// ids each version takes its policy from.
    pub fn bots(&self) -> Vec<serde_json::Value> {
        let reasons = self.retired_reasons();
        let retired = |id: &str| serde_json::json!(reasons.get(id));
        let mut out: Vec<serde_json::Value> = roster()
            .iter()
            .map(|s| serde_json::json!({ "id": s.id, "name": s.name, "blurb": s.blurb, "kind": "roster", "parents": [], "retired": retired(&s.id) }))
            .collect();
        let mut entrants: Vec<&Entrant> = self.entrants.iter().collect();
        entrants.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.extend(entrants.into_iter().map(|e| {
            serde_json::json!({
                "id": e.id,
                "name": e.name,
                "blurb": e.style().blurb,
                "kind": "entrant",
                "author": e.author,
                "parents": e.parents,
                "brain": if e.params.brain.is_empty() { "expander" } else { e.params.brain.as_str() },
                "own_build": e.program.is_some(),
                "created_at": e.created_at,
                "retired": retired(&e.id),
            })
        }));
        out
    }
}
