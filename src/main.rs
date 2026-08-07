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
//! Exit codes are the contract for automation: 0 the gate passed, 1 it failed, 2 the
//! tool could not run.

use aqs::cache::open_cache;
use aqs::delta::{compute_delta, DeltaRequest};
use aqs::explain::{lookup, RULE_DOCS};
use aqs::gate::{compute_gate, GateRequest};
use aqs::models::ScanReport;
use aqs::profile::{load_profile, Profile};
use aqs::render::{self, Style};
use aqs::{analyze, VERSION};
use clap::{Args, Parser, Subcommand};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
const EXIT_GATE_FAILED: u8 = 1;
const EXIT_ERROR: u8 = 2;

#[derive(Parser)]
#[command(
    name = "aqs",
    version = VERSION,
    about = "Deterministic, explainable code quality scoring for Python, TypeScript and JavaScript."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct ProfileArgs {
    /// Path to a config JSON file.
    #[arg(long = "profile")]
    profile_path: Option<PathBuf>,
}

#[derive(Args)]
struct ScanArgs {
    /// Files or directories to scan. Defaults to the CWD.
    paths: Vec<PathBuf>,
    /// Emit the full report as JSON.
    #[arg(long)]
    json: bool,
    /// With --json, embed fix guidance for every rule that fired.
    #[arg(long)]
    guidance: bool,
    /// Show every file with findings.
    #[arg(long)]
    all: bool,
    /// How many files to show.
    #[arg(long, short = 'n', default_value_t = 10)]
    limit: usize,
    /// Exit non-zero if the aggregate score falls below this.
    #[arg(long)]
    min_score: Option<f64>,
    /// Disable the .aqs/ report cache.
    #[arg(long)]
    no_cache: bool,
    #[command(flatten)]
    profile: ProfileArgs,
}

#[derive(Args)]
struct DeltaArgs {
    /// Git ref to compare against.
    #[arg(long, short, default_value = "HEAD")]
    baseline: String,
    /// Score the index instead of the working tree.
    #[arg(long)]
    staged: bool,
    /// Allowed aggregate score decrease before failing.
    #[arg(long, default_value_t = 0.0)]
    max_drop: f64,
    /// Compare against the merge base rather than the ref's tip.
    #[arg(long)]
    merge_base: bool,
    /// Emit the delta report as JSON.
    #[arg(long)]
    json: bool,
    /// Repository to inspect.
    #[arg(long)]
    path: Option<PathBuf>,
    #[command(flatten)]
    profile: ProfileArgs,
}

#[derive(Args)]
struct GateArgs {
    /// Branch or ref to compare the whole repository against.
    #[arg(long, short, default_value = "origin/develop")]
    against: String,
    /// Allowed repository score drop. 0 means it may never go down.
    #[arg(long, default_value_t = 0.0)]
    tolerance: f64,
    /// Compare against the ref's tip instead of the merge base.
    #[arg(long)]
    no_merge_base: bool,
    /// Emit the gate report as JSON.
    #[arg(long)]
    json: bool,
    /// Repository to inspect.
    #[arg(long)]
    path: Option<PathBuf>,
    /// Disable the .aqs/ report cache.
    #[arg(long)]
    no_cache: bool,
    #[command(flatten)]
    profile: ProfileArgs,
}

#[derive(Args)]
struct CheckArgs {
    /// Files or directories to check. Defaults to the CWD.
    paths: Vec<PathBuf>,
    /// Exit non-zero if anything is found.
    #[arg(long)]
    fail_on_findings: bool,
    #[command(flatten)]
    profile: ProfileArgs,
}

#[derive(Args)]
struct ExplainArgs {
    /// Rule name, e.g. 'Bumpy Road Ahead'.
    rule: Option<String>,
    /// List every rule and its weight.
    #[arg(long)]
    list: bool,
    /// List every configurable threshold.
    #[arg(long)]
    thresholds: bool,
    /// Emit the guidance as JSON.
    #[arg(long)]
    json: bool,
    #[command(flatten)]
    profile: ProfileArgs,
}

#[derive(Subcommand)]
enum Command {
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

fn terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
}

fn style() -> Style {
    Style::new(std::io::stdout().is_terminal(), terminal_width())
}

fn print_json(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).expect("report is serialisable"));
}

/// Fix guidance for just the rules that actually fired.
fn guidance_for(report: &ScanReport) -> Value {
    let present: BTreeSet<&str> = report
        .files
        .iter()
        .flat_map(|file| file.all_findings())
        .map(|finding| finding.category.as_str())
        .collect();
    let mut map = Map::new();
    for doc in RULE_DOCS {
        if present.contains(doc.category) {
            map.insert(
                doc.category.to_string(),
                json!({ "summary": doc.summary, "why": doc.why, "fix": doc.fix }),
            );
        }
    }
    Value::Object(map)
}

fn load(root: &Path, explicit: Option<&Path>) -> Result<Profile, ExitCode> {
    load_profile(root, explicit).map_err(|error| {
        eprintln!("config error: {error}");
        ExitCode::from(EXIT_ERROR)
    })
}

fn resolve_targets(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, ExitCode> {
    let targets = if paths.is_empty() {
        vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))]
    } else {
        paths
    };
    for target in &targets {
        if !target.exists() {
            eprintln!("error: no such path: {}", target.display());
            return Err(ExitCode::from(EXIT_ERROR));
        }
    }
    Ok(targets)
}

/// The directory that config discovery and relative display paths hang off.
///
/// `Path::new("thing.py").parent()` is `Some("")`, not `None`, so an `unwrap_or(".")`
/// never fires for a bare filename and the base silently becomes the empty path. That
/// made `aqs check thing.py` skip `.aqs.json` entirely and print absolute paths — on
/// exactly the single-file invocation the editor and agent loops use.
fn base_of(targets: &[PathBuf]) -> PathBuf {
    let first = &targets[0];
    if first.is_dir() {
        return first.clone();
    }
    match first.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn run_scan(args: ScanArgs) -> Result<ExitCode, ExitCode> {
    let targets = resolve_targets(args.paths)?;
    let base = base_of(&targets);
    let loaded = load(&base, args.profile.profile_path.as_deref())?;
    let mut cache = open_cache(Some(&base), &loaded, !args.no_cache);
    let report = analyze::scan(&targets, &loaded, Some(&base), cache.as_mut());

    if args.json {
        let mut value = serde_json::to_value(&report).expect("serialisable");
        if args.guidance {
            value
                .as_object_mut()
                .expect("report is an object")
                .insert("guidance".into(), guidance_for(&report));
        }
        print_json(&value);
    } else {
        render::render_scan(&report, &style(), args.limit, args.all);
    }

    let Some(minimum) = args.min_score else {
        return Ok(ExitCode::from(EXIT_OK));
    };
    if report.score >= minimum {
        return Ok(ExitCode::from(EXIT_OK));
    }
    if !args.json {
        eprintln!("score {:.2} is below the required {minimum:.2}", report.score);
    }
    Ok(ExitCode::from(EXIT_GATE_FAILED))
}

fn run_delta(args: DeltaArgs) -> Result<ExitCode, ExitCode> {
    let root = args.path.unwrap_or_else(current_dir);
    let loaded = load(&root, args.profile.profile_path.as_deref())?;
    let mut cache = open_cache(None, &loaded, false);
    let report = compute_delta(
        DeltaRequest {
            root: &root,
            profile: &loaded,
            baseline: &args.baseline,
            staged: args.staged,
            merge_base: args.merge_base,
        },
        cache.as_mut(),
    )
    .map_err(git_error)?;

    if args.json {
        print_json(&serde_json::to_value(&report).expect("serialisable"));
    } else {
        render::render_delta(&report, &style(), args.max_drop);
    }

    let failed = report.score_change < -args.max_drop.abs();
    Ok(ExitCode::from(if failed { EXIT_GATE_FAILED } else { EXIT_OK }))
}

fn run_gate(args: GateArgs) -> Result<ExitCode, ExitCode> {
    let root = args.path.unwrap_or_else(current_dir);
    let loaded = load(&root, args.profile.profile_path.as_deref())?;
    let repo_root = aqs::gitio::repo_root(&root).ok();
    let mut cache = open_cache(repo_root.as_deref(), &loaded, !args.no_cache);
    let report = compute_gate(
        GateRequest {
            root: &root,
            profile: &loaded,
            against: &args.against,
            tolerance: args.tolerance,
            merge_base: !args.no_merge_base,
        },
        cache.as_mut(),
    )
    .map_err(git_error)?;

    if args.json {
        print_json(&serde_json::to_value(&report).expect("serialisable"));
    } else {
        render::render_gate(&report, &style());
    }
    Ok(ExitCode::from(if report.passed() { EXIT_OK } else { EXIT_GATE_FAILED }))
}

fn run_check(args: CheckArgs) -> Result<ExitCode, ExitCode> {
    let targets = resolve_targets(args.paths)?;
    let base = base_of(&targets);
    let loaded = load(&base, args.profile.profile_path.as_deref())?;
    let mut cache = open_cache(None, &loaded, false);
    let report = analyze::scan(&targets, &loaded, Some(&base), cache.as_mut());
    render::render_check(&report);

    let failed = args.fail_on_findings && report.findings_count() > 0;
    Ok(ExitCode::from(if failed { EXIT_GATE_FAILED } else { EXIT_OK }))
}

fn thresholds_as_json(profile: &Profile, keys: &[&str]) -> Value {
    let mut map = Map::new();
    for key in keys {
        map.insert((*key).to_string(), json!(profile.threshold(key)));
    }
    Value::Object(map)
}

/// Every rule as JSON, for `--list --json`.
fn rules_as_json(profile: &Profile) -> Value {
    let mut map = Map::new();
    for doc in RULE_DOCS {
        map.insert(
            doc.category.to_string(),
            json!({
                "summary": doc.summary,
                "why": doc.why,
                "fix": doc.fix,
                "weight": profile.rule(doc.category).weight,
                "thresholds": thresholds_as_json(profile, doc.thresholds),
            }),
        );
    }
    Value::Object(map)
}

fn one_rule_as_json(profile: &Profile, doc: &aqs::explain::RuleDoc) -> Value {
    json!({
        "category": doc.category,
        "summary": doc.summary,
        "why": doc.why,
        "fix": doc.fix,
        "thresholds": thresholds_as_json(profile, doc.thresholds),
    })
}

fn list_thresholds(profile: &Profile, as_json: bool, style: &Style) {
    if as_json {
        let keys: Vec<&str> = profile.thresholds.keys().map(String::as_str).collect();
        print_json(&thresholds_as_json(profile, &keys));
    } else {
        render::render_thresholds(profile, style);
    }
}

fn list_rules(profile: &Profile, as_json: bool, style: &Style) {
    if as_json {
        print_json(&rules_as_json(profile));
    } else {
        render::render_rule_list(profile, style);
    }
}

fn run_explain(args: ExplainArgs) -> Result<ExitCode, ExitCode> {
    let loaded = load(&current_dir(), args.profile.profile_path.as_deref())?;
    let style = style();

    if args.thresholds {
        list_thresholds(&loaded, args.json, &style);
        return Ok(ExitCode::from(EXIT_OK));
    }
    if args.list || args.rule.is_none() {
        list_rules(&loaded, args.json, &style);
        return Ok(ExitCode::from(EXIT_OK));
    }

    let query = args.rule.expect("checked above");
    let Some(doc) = lookup(&query) else {
        eprintln!("error: unknown rule {query:?}. Run `aqs explain --list`.");
        return Ok(ExitCode::from(EXIT_ERROR));
    };

    if args.json {
        print_json(&one_rule_as_json(&loaded, doc));
    } else {
        render::render_explain(doc, &loaded, &style);
    }
    Ok(ExitCode::from(EXIT_OK))
}

fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn git_error(error: anyhow::Error) -> ExitCode {
    eprintln!("git error: {error}");
    ExitCode::from(EXIT_ERROR)
}

fn run() -> Result<ExitCode, ExitCode> {
    match Cli::parse().command {
        Command::Scan(args) => run_scan(args),
        Command::Delta(args) => run_delta(args),
        Command::Gate(args) => run_gate(args),
        Command::Check(args) => run_check(args),
        Command::Explain(args) => run_explain(args),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(code) => code,
    }
}
