//! Hexarena bot SDK. A bot sees only a fog-filtered `View` and returns one `Action` per tick.

pub use hexarena_protocol::*;
use std::collections::HashMap;

pub trait Bot {
    fn name(&self) -> &str;
    fn act(&mut self, view: &View) -> Action;
}

fn terrain_at(view: &View, h: Hex) -> Option<Terrain> {
    let w = 2 * view.radius + 1;
    if h.q.abs() > view.radius || h.r.abs() > view.radius || (h.q + h.r).abs() > view.radius {
        return None;
    }
    view.terrain[((h.r + view.radius) * w + h.q + view.radius) as usize]
}

/// Starter bot: attacks the weakest adjacent non-owned hex it can beat, otherwise
/// pushes surplus troops toward the nearest hex it does not own. Uses only its View.
#[derive(Default)]
pub struct Expander {
    seen: HashMap<Hex, VisibleCell>,
}

impl Bot for Expander {
    fn name(&self) -> &str {
        "expander"
    }

    fn act(&mut self, view: &View) -> Action {
        for c in &view.visible {
            self.seen.insert(c.hex, c.clone());
        }
        let me = view.player;
        let mine: Vec<&VisibleCell> = view
            .visible
            .iter()
            .filter(|c| c.owner == Some(me))
            .collect();
        let mut best: Option<(i64, Action)> = None;
        for c in &mine {
            if c.troops < 2 {
                continue;
            }
            for nb in c.hex.neighbors() {
                let Some(t) = terrain_at(view, nb) else {
                    continue;
                };
                if !t.passable() {
                    continue;
                }
                let target = self.seen.get(&nb);
                let (owner, def) = target.map(|v| (v.owner, v.troops)).unwrap_or((None, 0));
                if owner == Some(me) {
                    continue;
                }
                let send = c.troops - 1;
                let pct = [60, 80, 100, 130, 170][(t.height + 2) as usize];
                if send * 100 <= def * pct {
                    continue;
                }
                let score = (send * 100 - def * pct) as i64 + if owner.is_some() { 500 } else { 0 };
                let a = Action::Move {
                    from: c.hex,
                    to: nb,
                    troops: send,
                };
                if best.as_ref().map_or(true, |(s, _)| score > *s) {
                    best = Some((score, a));
                }
            }
        }
        best.map(|(_, a)| a).unwrap_or(Action::Pass)
    }
}
