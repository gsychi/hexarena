//! Brains: whole CPU strategies. A genome's `brain` field picks which one plays it.
//!
//! To add a brain, write a module here with a type implementing [`Bot`] that is built
//! from a [`Style`], add it to [`BRAINS`] and to the match in [`build`]. Read tunable
//! numbers with [`BotParams::knob`] so the league can evolve them.
//!
//! [`BotParams::knob`]: crate::BotParams::knob

use crate::{Bot, Expander, Style};

/// The brain genomes play when they don't name one.
pub const DEFAULT: &str = "expander";

/// Every brain id [`build`] accepts.
pub const BRAINS: &[&str] = &[DEFAULT];

/// The bot that plays `style`, or an error naming an unknown brain.
pub fn build(style: Style) -> Result<Box<dyn Bot>, String> {
    match style.params.brain.as_str() {
        DEFAULT => Ok(Box::new(Expander::new(style))),
        other => Err(format!(
            "unknown brain '{other}' (known: {})",
            BRAINS.join(", ")
        )),
    }
}
