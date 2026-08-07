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
use aqs::delta::compute_delta;
use aqs::explain::{lookup, RULE_DOCS};
use aqs::gate::compute_gate;
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

#[derive(Subcommand)]
enum Command {
    /// Score a tree and report the lowest-scoring files.
    Scan {
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
    },
    /// Compare changed files against a git baseline. Fails on regression.
    Delta {
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
    },
    /// Enforce that the whole repository's score never goes down.
    Gate {
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
    },
    /// Emit findings as lint-style `path:line:col: rule: details` lines.
    Check {
        /// Files or directories to check. Defaults to the CWD.
        paths: Vec<PathBuf>,
        /// Exit non-zero if anything is found.
        #[arg(long)]
        fail_on_findings: bool,
        #[command(flatten)]
        profile: ProfileArgs,
    },
    /// Explain what a rule means, why it matters, and how to fix it.
    Explain {
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
    },
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

fn base_of(targets: &[PathBuf]) -> PathBuf {
    let first = &targets[0];
    if first.is_dir() {
        first.clone()
    } else {
        first.parent().unwrap_or(Path::new(".")).to_path_buf()
    }
}

fn run() -> Result<ExitCode, ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Command::Scan {
            paths,
            json,
            guidance,
            all,
            limit,
            min_score,
            no_cache,
            profile,
        } => {
            let targets = resolve_targets(paths)?;
            let base = base_of(&targets);
            let loaded = load(&base, profile.profile_path.as_deref())?;
            let mut cache = open_cache(Some(&base), &loaded, !no_cache);
            let report = analyze::scan(&targets, &loaded, Some(&base), cache.as_mut());

            if json {
                let mut value = serde_json::to_value(&report).expect("serialisable");
                if guidance {
                    value
                        .as_object_mut()
                        .expect("report is an object")
                        .insert("guidance".into(), guidance_for(&report));
                }
                print_json(&value);
            } else {
                render::render_scan(&report, &style(), limit, all);
            }

            if let Some(minimum) = min_score {
                if report.score < minimum {
                    if !json {
                        eprintln!(
                            "score {:.2} is below the required {:.2}",
                            report.score, minimum
                        );
                    }
                    return Ok(ExitCode::from(EXIT_GATE_FAILED));
                }
            }
            Ok(ExitCode::from(EXIT_OK))
        }

        Command::Delta {
            baseline,
            staged,
            max_drop,
            merge_base,
            json,
            path,
            profile,
        } => {
            let root = path.unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            });
            let loaded = load(&root, profile.profile_path.as_deref())?;
            let mut cache = open_cache(None, &loaded, false);
            let report =
                compute_delta(&root, &loaded, &baseline, staged, merge_base, cache.as_mut())
                    .map_err(|error| {
                        eprintln!("git error: {error}");
                        ExitCode::from(EXIT_ERROR)
                    })?;

            if json {
                print_json(&serde_json::to_value(&report).expect("serialisable"));
            } else {
                render::render_delta(&report, &style(), max_drop);
            }

            Ok(ExitCode::from(if report.score_change < -max_drop.abs() {
                EXIT_GATE_FAILED
            } else {
                EXIT_OK
            }))
        }

        Command::Gate {
            against,
            tolerance,
            no_merge_base,
            json,
            path,
            no_cache,
            profile,
        } => {
            let root = path.unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            });
            let loaded = load(&root, profile.profile_path.as_deref())?;
            let repo_root = aqs::gitio::repo_root(&root).ok();
            let mut cache = open_cache(repo_root.as_deref(), &loaded, !no_cache);
            let report = compute_gate(
                &root,
                &loaded,
                &against,
                tolerance,
                !no_merge_base,
                cache.as_mut(),
            )
            .map_err(|error| {
                eprintln!("git error: {error}");
                ExitCode::from(EXIT_ERROR)
            })?;

            if json {
                print_json(&serde_json::to_value(&report).expect("serialisable"));
            } else {
                render::render_gate(&report, &style());
            }

            Ok(ExitCode::from(if report.passed() { EXIT_OK } else { EXIT_GATE_FAILED }))
        }

        Command::Check { paths, fail_on_findings, profile } => {
            let targets = resolve_targets(paths)?;
            let base = base_of(&targets);
            let loaded = load(&base, profile.profile_path.as_deref())?;
            let mut cache = open_cache(None, &loaded, false);
            let report = analyze::scan(&targets, &loaded, Some(&base), cache.as_mut());
            render::render_check(&report);

            Ok(ExitCode::from(
                if fail_on_findings && report.findings_count() > 0 {
                    EXIT_GATE_FAILED
                } else {
                    EXIT_OK
                },
            ))
        }

        Command::Explain { rule, list, thresholds, json, profile } => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let loaded = load(&cwd, profile.profile_path.as_deref())?;
            let style = style();

            if thresholds {
                if json {
                    let map: Map<String, Value> = loaded
                        .thresholds
                        .iter()
                        .map(|(key, value)| (key.clone(), json!(value)))
                        .collect();
                    print_json(&Value::Object(map));
                } else {
                    render::render_thresholds(&loaded, &style);
                }
                return Ok(ExitCode::from(EXIT_OK));
            }

            if list || rule.is_none() {
                if json {
                    let mut map = Map::new();
                    for doc in RULE_DOCS {
                        let mut threshold_map = Map::new();
                        for key in doc.thresholds {
                            threshold_map
                                .insert((*key).to_string(), json!(loaded.threshold(key)));
                        }
                        map.insert(
                            doc.category.to_string(),
                            json!({
                                "summary": doc.summary,
                                "why": doc.why,
                                "fix": doc.fix,
                                "weight": loaded.rule(doc.category).weight,
                                "thresholds": Value::Object(threshold_map),
                            }),
                        );
                    }
                    print_json(&Value::Object(map));
                } else {
                    render::render_rule_list(&loaded, &style);
                }
                return Ok(ExitCode::from(EXIT_OK));
            }

            let query = rule.expect("checked above");
            let Some(doc) = lookup(&query) else {
                eprintln!("error: unknown rule {query:?}. Run `aqs explain --list`.");
                return Ok(ExitCode::from(EXIT_ERROR));
            };

            if json {
                let mut threshold_map = Map::new();
                for key in doc.thresholds {
                    threshold_map.insert((*key).to_string(), json!(loaded.threshold(key)));
                }
                print_json(&json!({
                    "category": doc.category,
                    "summary": doc.summary,
                    "why": doc.why,
                    "fix": doc.fix,
                    "thresholds": Value::Object(threshold_map),
                }));
            } else {
                render::render_explain(doc, &loaded, &style);
            }
            Ok(ExitCode::from(EXIT_OK))
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(code) => code,
    }
}
