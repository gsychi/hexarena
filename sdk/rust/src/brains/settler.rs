//! Villages for brains that only fight. Plays the inner brain every tick; at most once
//! every `village_every` ticks it spends the tick on a Village instead, on the safe
//! interior hex with the most troops, if one can pay for it. A Village repays its 75
//! troops in 75 ticks and keeps paying, so the tick it costs is usually worth it.
//!
//! Knobs: `village_every` (minimum ticks between Villages, 0 = never), `village_keep`
//! (troops left on the hex beyond the one that must stay), `village_cap` (most Villages).

use crate::{Bot, Style};
use hexarena_protocol::{Action, Building, Hex, PlayerCommand, RulesConfig, View};

pub struct Settler {
    inner: Box<dyn Bot>,
    rules: RulesConfig,
    every: u32,
    keep: u32,
    cap: usize,
    last: Option<u32>,
}

impl Settler {
    pub fn wrap(inner: Box<dyn Bot>, style: &Style) -> Settler {
        let k = |n: &str, d: i64| style.params.knob(n, d).max(0);
        Settler {
            inner,
            rules: RulesConfig::default(),
            every: k("village_every", 8) as u32,
            keep: k("village_keep", 0) as u32,
            cap: k("village_cap", 200) as usize,
            last: None,
        }
    }

    fn site(&self, view: &View, busy: Option<Hex>) -> Option<Hex> {
        let r = view.radius;
        let w = (2 * r + 1) as usize;
        let slot = |h: Hex| {
            (h.q.abs() <= r && h.r.abs() <= r && (h.q + h.r).abs() <= r)
                .then(|| ((h.r + r) as usize) * w + (h.q + r) as usize)
        };
        let mut owner = vec![None; w * w];
        let mut villages = 0;
        for c in &view.visible_hexes {
            if let Some(i) = slot(c.hex) {
                owner[i] = c.owner;
            }
            if c.owner == Some(view.you)
                && c.building.as_ref().is_some_and(|b| b.kind == Building::Village)
            {
                villages += 1;
            }
        }
        if villages >= self.cap {
            return None;
        }
        let passable = |i: usize| view.public_terrain.get(i).copied().flatten().is_some_and(|t| t.passable());
        let safe = |h: Hex| view.storm_radius.is_none_or(|s| h.distance(Hex::new(0, 0)) < s);
        let need = self.rules.build_cost(Building::Village, false).troops + 1 + self.keep;
        view.visible_hexes
            .iter()
            .filter(|c| {
                c.owner == Some(view.you)
                    && c.building.is_none()
                    && Some(c.hex) != busy
                    && c.troops >= need
                    && safe(c.hex)
            })
            .filter(|c| {
                c.hex.neighbors().into_iter().all(|n| match slot(n) {
                    Some(i) => !passable(i) || owner[i] == Some(view.you),
                    None => true,
                })
            })
            .max_by_key(|c| (c.troops, std::cmp::Reverse(c.hex)))
            .map(|c| c.hex)
    }
}

impl Bot for Settler {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if let Some(r) = &view.rules {
            self.rules = r.clone();
        }
        let mut cmd = self.inner.act(view);
        if self.every == 0 || self.last.is_some_and(|t| view.tick < t + self.every) {
            return cmd;
        }
        let busy = match &cmd.action {
            Action::Move { from, .. } => Some(*from),
            Action::Gather { .. } | Action::Build { .. } => return cmd,
            Action::NoOp => None,
        };
        if let Some(at) = self.site(view, busy) {
            cmd.action = Action::Build { at, building: Building::Village };
            self.last = Some(view.tick);
        }
        cmd
    }
}
