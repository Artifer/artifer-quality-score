//! Command line interface.
//!
//! Five commands, matching the moments you actually want a quality number:
//!
//! ```text
//! aqs scan      what is the state of this code
//! aqs delta     did this change make it worse   (the TDD / pre-commit loop)
//! aqs gate      may this merge into develop     (whole-repo, no-regression rule)
//! aqs check     lint-style lines for an editor or a grep
//! aqs explain   what does this rule mean and how do I fix it
//! ```
//!
//! `delta` and `gate` are not the same check. `delta` compares only changed files and is
//! fast enough to run on save. `gate` scores both trees in full, which is the only way to
//! catch a repository-score drop caused by LoC weight shift.
//!
//! Exit codes are the contract for automation: 0 the gate passed, 1 it failed, 2 the tool
//! could not run.

pub mod args;
pub mod commands;
pub mod output;

use args::{Cli, Command};
use clap::Parser;
use commands::{run_check, run_delta, run_explain, run_gate, run_scan};
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
const EXIT_GATE_FAILED: u8 = 1;
const EXIT_ERROR: u8 = 2;

pub fn run() -> Result<ExitCode, ExitCode> {
    match Cli::parse().command {
        Command::Scan(args) => run_scan(args),
        Command::Delta(args) => run_delta(args),
        Command::Gate(args) => run_gate(args),
        Command::Check(args) => run_check(args),
        Command::Explain(args) => run_explain(args),
    }
}
