//! Artifer Quality Score — deterministic, explainable code quality scoring.
//!
//! A 1.0-10.0 score per file, derived from a catalogue of structural code smells, built
//! for two jobs: a fast quality read during a TDD loop, and a gradient an LLM can
//! actually climb when asked to improve code it just wrote.
//!
//! The rule taxonomy follows CodeScene's published code-health factors. The penalty
//! weights are our own — theirs are proprietary. "CodeHealth" is CodeScene's trademark;
//! this is a separate tool and its numbers are not comparable to theirs.
//!
//! This is a port of the Python reference implementation in `../src/aqs`, which is
//! retained as a differential-testing oracle: any disagreement between the two is a bug.

pub mod analyze;
pub mod cache;
pub mod cli;
pub mod clones;
pub mod delta;
pub mod detectors;
pub mod difflib;
pub mod directives;
pub mod explain;
pub mod gate;
pub mod gitio;
pub mod glob;
pub mod languages;
pub mod metrics;
pub mod models;
pub mod profile;
pub mod render;
pub mod scoring;
pub mod sfc;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
