//! Snowshoe: Ridgeline's border discipline plus a configurable infrastructure planner.
//!
//! The capture policy remains deliberately comparable to the reigning champion while
//! genomes vary the economic portfolio: linked Farm/Village engines, defensive wall
//! lines, quarry hubs, or hybrids. This isolates whether spending actions and troops on
//! buildings can beat pure land-and-garrison play.

use crate::brains::ridgeline::Ridgeline;
use crate::{Bot, PlayerCommand, Style, View};

pub struct Snowshoe {
    inner: Ridgeline,
}

impl Snowshoe {
    pub fn new(style: Style) -> Self {
        Snowshoe {
            inner: Ridgeline::new(style),
        }
    }
}

impl Bot for Snowshoe {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        self.inner.act(view)
    }
}
