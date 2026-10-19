//! Bastion: a Ridgeline-derived gate specialist with an explicit kin treaty.
//!
//! Mirrored league gates seat three copies of each genome. Fighting an identical copy
//! spends troops and actions without improving the genome's average placement. Bastion
//! recognizes its own publicly visible seat-name family and presents that land to its
//! Ridgeline tactical core as prohibitively expensive neutral ground. The core therefore
//! keeps its safety model, supply relays and capture selection against every real rival,
//! but does not churn borders shared with another Bastion copy.

use super::ridgeline::Ridgeline;
use crate::{Bot, PlayerCommand, Style, View};

pub struct Bastion {
    name: String,
    family: String,
    family_prefix: String,
    treaty_until: u32,
    inner: Ridgeline,
}

impl Bastion {
    pub fn new(style: Style) -> Self {
        let treaty_until = style.params.knob("treaty_until", 3001).max(0) as u32;
        Self {
            name: style.id.clone(),
            family: style.name.clone(),
            family_prefix: format!("{} ", style.name),
            treaty_until,
            inner: Ridgeline::new(style),
        }
    }

    fn same_family(&self, name: &str) -> bool {
        name == self.family
            || name
                .strip_prefix(&self.family_prefix)
                .is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                })
    }
}

impl Bot for Bastion {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if view.tick >= self.treaty_until {
            return self.inner.act(view);
        }
        let kin: Vec<bool> = view
            .public_players
            .iter()
            .map(|player| player.id != view.you && self.same_family(&player.name))
            .collect();
        let mut shadow = view.clone();
        for cell in &mut shadow.visible_hexes {
            if cell
                .owner
                .is_some_and(|owner| kin.get(owner as usize).copied().unwrap_or(false))
            {
                // Neutral means the Ridgeline safety model does not garrison against kin.
                // The impossible defense means its capture planner never attacks them.
                cell.owner = None;
                cell.troops = 1_000_000;
                cell.wall_milli = 5_000;
            }
        }
        self.inner.act(&shadow)
    }
}
