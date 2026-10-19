//! Brains: whole CPU strategies. A genome's `brain` field picks which one plays it.
//!
//! To add a brain, write a module here with a type implementing [`Bot`] that is built
//! from a [`Style`], add it to [`BRAINS`] and to the match in [`build`]. Read tunable
//! numbers with [`BotParams::knob`] when the bot is built, so the league can find and
//! evolve them.
//!
//! [`BotParams::knob`]: crate::BotParams::knob

pub mod bastion;
pub mod mosaic;
pub mod predator;
pub mod relay;
pub mod ridgeline;

use crate::process::ProcessBot;
use crate::{Bot, BotParams, Expander, Style, KNOB_LOG};
use std::collections::BTreeMap;

/// The brain genomes play when they don't name one.
pub const DEFAULT: &str = "expander";

/// Every brain id [`build`] accepts.
pub const BRAINS: &[&str] = &[
    DEFAULT,
    "relay",
    "predator",
    "ridgeline",
    "mosaic",
    "bastion",
];

/// The bot that plays `style`, or an error naming an unknown brain.
pub fn build(style: Style) -> Result<Box<dyn Bot>, String> {
    if let Some(program) = &style.program {
        return Ok(Box::new(ProcessBot::spawn(program, &style)?));
    }
    match style.params.brain.as_str() {
        DEFAULT => Ok(Box::new(Expander::new(style))),
        "bastion" => Ok(Box::new(bastion::Bastion::new(style))),
        "mosaic" => Ok(Box::new(mosaic::Mosaic::new(style))),
        "relay" => Ok(Box::new(relay::Relay::new(style))),
        "predator" => Ok(Box::new(predator::Predator::new(style))),
        "ridgeline" => Ok(Box::new(ridgeline::Ridgeline::new(style))),
        other => Err(format!(
            "unknown brain '{other}' (known: {})",
            BRAINS.join(", ")
        )),
    }
}

/// Adds every knob `params.brain` reads, at its default, to `params.knobs`, so the league
/// can mutate knobs a genome never set. Values already set are kept.
pub fn complete_knobs(params: &mut BotParams) -> Result<(), String> {
    KNOB_LOG.with(|log| *log.borrow_mut() = Some(BTreeMap::new()));
    let style = Style {
        id: String::new(),
        name: String::new(),
        blurb: String::new(),
        params: params.clone(),
        program: None,
    };
    let built = build(style).map(drop);
    let seen = KNOB_LOG
        .with(|log| log.borrow_mut().take())
        .unwrap_or_default();
    built?;
    for (k, v) in seen {
        params.knobs.entry(k).or_insert(v);
    }
    Ok(())
}
