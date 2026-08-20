//! The command-line surface.
//!
//! One `Args` struct per subcommand, so each handler takes a single argument. Passing
//! eight loose parameters is the smell this tool reports.

use crate::VERSION;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "aqs",
    version = VERSION,
    about = "Deterministic, explainable code quality scoring for Python, TypeScript, JavaScript, Rust, Go, Ruby, Java, PHP, Vue and Svelte."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct ProfileArgs {
    /// Path to a config JSON file.
    #[arg(long = "profile")]
    pub profile_path: Option<PathBuf>,
}

#[derive(Args)]
pub struct ScanArgs {
    /// Files or directories to scan. Defaults to the CWD.
    pub paths: Vec<PathBuf>,
    /// Emit the full report as JSON.
    #[arg(long)]
    pub json: bool,
    /// With --json, embed fix guidance for every rule that fired.
    #[arg(long)]
    pub guidance: bool,
    /// Show every file with findings.
    #[arg(long)]
    pub all: bool,
    /// How many files to show.
    #[arg(long, short = 'n', default_value_t = 10)]
    pub limit: usize,
    /// Exit non-zero if the aggregate score falls below this.
    #[arg(long)]
    pub min_score: Option<f64>,
    /// Disable the .aqs/ report cache.
    #[arg(long)]
    pub no_cache: bool,
    /// Also scan linked git worktrees found inside the target.
    #[arg(long)]
    pub include_worktrees: bool,
    #[command(flatten)]
    pub profile: ProfileArgs,
}

#[derive(Args)]
pub struct DeltaArgs {
    /// Git ref to compare against.
    #[arg(long, short, default_value = "HEAD")]
    pub baseline: String,
    /// Score the index instead of the working tree.
    #[arg(long)]
    pub staged: bool,
    /// Allowed aggregate score decrease before failing.
    #[arg(long, default_value_t = 0.0)]
    pub max_drop: f64,
    /// Compare against the merge base rather than the ref's tip.
    #[arg(long)]
    pub merge_base: bool,
    /// Emit the delta report as JSON.
    #[arg(long)]
    pub json: bool,
    /// Repository to inspect.
    #[arg(long)]
    pub path: Option<PathBuf>,
    #[command(flatten)]
    pub profile: ProfileArgs,
}

#[derive(Args)]
pub struct GateArgs {
    /// Branch or ref to compare the whole repository against.
    #[arg(long, short, default_value = "origin/develop")]
    pub against: String,
    /// Allowed repository score drop. 0 means it may never go down.
    #[arg(long, default_value_t = 0.0)]
    pub tolerance: f64,
    /// Compare against the ref's tip instead of the merge base.
    #[arg(long)]
    pub no_merge_base: bool,
    /// Emit the gate report as JSON.
    #[arg(long)]
    pub json: bool,
    /// Repository to inspect.
    #[arg(long)]
    pub path: Option<PathBuf>,
    /// Disable the .aqs/ report cache.
    #[arg(long)]
    pub no_cache: bool,
    #[command(flatten)]
    pub profile: ProfileArgs,
}

#[derive(Args)]
pub struct CheckArgs {
    /// Files or directories to check. Defaults to the CWD.
    pub paths: Vec<PathBuf>,
    /// Exit non-zero if anything is found.
    #[arg(long)]
    pub fail_on_findings: bool,
    /// Also check linked git worktrees found inside the target.
    #[arg(long)]
    pub include_worktrees: bool,
    #[command(flatten)]
    pub profile: ProfileArgs,
}

#[derive(Args)]
pub struct ExplainArgs {
    /// Rule name, e.g. 'Bumpy Road Ahead'.
    pub rule: Option<String>,
    /// List every rule and its weight.
    #[arg(long)]
    pub list: bool,
    /// List every configurable threshold.
    #[arg(long)]
    pub thresholds: bool,
    /// Emit the guidance as JSON.
    #[arg(long)]
    pub json: bool,
    #[command(flatten)]
    pub profile: ProfileArgs,
}

#[derive(Subcommand)]
pub enum Command {
    /// Score a tree and report the lowest-scoring files.
    Scan(ScanArgs),
    /// Compare changed files against a git baseline. Fails on regression.
    Delta(DeltaArgs),
    /// Enforce that the whole repository's score never goes down.
    Gate(GateArgs),
    /// Emit findings as lint-style `path:line:col: rule: details` lines.
    Check(CheckArgs),
    /// Explain what a rule means, why it matters, and how to fix it.
    Explain(ExplainArgs),
}
