//! One handler per subcommand.

use super::args::{CheckArgs, DeltaArgs, ExplainArgs, GateArgs, ScanArgs};
use super::output::{
    guidance_for, one_rule_as_json, print_json, report_as_json, rules_as_json, style,
    thresholds_as_json,
};
use super::{EXIT_ERROR, EXIT_GATE_FAILED, EXIT_OK};
use crate::analyze;
use crate::cache::open_cache;
use crate::delta::{compute_delta, DeltaRequest};
use crate::explain::lookup;
use crate::gate::{compute_gate, GateRequest};
use crate::profile::{load_profile, Profile};
use crate::render::{self, Style};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn load(root: &Path, explicit: Option<&Path>) -> Result<Profile, ExitCode> {
    load_profile(root, explicit).map_err(|error| {
        eprintln!("config error: {error}");
        ExitCode::from(EXIT_ERROR)
    })
}

pub(super) fn resolve_targets(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, ExitCode> {
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
pub(super) fn base_of(targets: &[PathBuf]) -> PathBuf {
    let first = &targets[0];
    if first.is_dir() {
        return first.clone();
    }
    match first.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Resolve the targets, find the directory config hangs off, and load the profile.
///
/// `scan` and `check` open with exactly this, and the flag has to be folded in at both.
struct Target {
    paths: Vec<PathBuf>,
    base: PathBuf,
    profile: Profile,
}

fn prepare(
    paths: Vec<PathBuf>,
    args: &super::args::ProfileArgs,
    include_worktrees: bool,
) -> Result<Target, ExitCode> {
    let paths = resolve_targets(paths)?;
    let base = base_of(&paths);
    let mut profile = load(&base, args.profile_path.as_deref())?;
    // The flag turns worktrees on; it never turns them off, so a config that asked for
    // them is not silently overridden by its absence.
    profile.scan_worktrees |= include_worktrees;
    Ok(Target {
        paths,
        base,
        profile,
    })
}

pub fn run_scan(args: ScanArgs) -> Result<ExitCode, ExitCode> {
    let Target {
        paths: targets,
        base,
        profile: loaded,
    } = prepare(args.paths, &args.profile, args.include_worktrees)?;
    let mut cache = open_cache(Some(&base), &loaded, !args.no_cache);
    let report = analyze::scan(&targets, &loaded, Some(&base), cache.as_mut());

    if args.json {
        let mut value = report_as_json(&report);
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

    if report.files.iter().any(|f| f.directive_error.is_some()) {
        // A directive that does not work is worse than no directive, so this fails even
        // when the score itself is fine.
        return Ok(ExitCode::from(EXIT_ERROR));
    }

    let Some(minimum) = args.min_score else {
        return Ok(ExitCode::from(EXIT_OK));
    };
    if report.score >= minimum {
        return Ok(ExitCode::from(EXIT_OK));
    }
    if !args.json {
        eprintln!(
            "score {:.2} is below the required {minimum:.2}",
            report.score
        );
    }
    Ok(ExitCode::from(EXIT_GATE_FAILED))
}

pub fn run_delta(args: DeltaArgs) -> Result<ExitCode, ExitCode> {
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
    Ok(ExitCode::from(if failed {
        EXIT_GATE_FAILED
    } else {
        EXIT_OK
    }))
}

pub fn run_gate(args: GateArgs) -> Result<ExitCode, ExitCode> {
    let root = args.path.unwrap_or_else(current_dir);
    let loaded = load(&root, args.profile.profile_path.as_deref())?;
    let repo_root = crate::gitio::repo_root(&root).ok();
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
    Ok(ExitCode::from(if report.passed() {
        EXIT_OK
    } else {
        EXIT_GATE_FAILED
    }))
}

pub fn run_check(args: CheckArgs) -> Result<ExitCode, ExitCode> {
    let Target {
        paths: targets,
        base,
        profile: loaded,
    } = prepare(args.paths, &args.profile, args.include_worktrees)?;
    let mut cache = open_cache(None, &loaded, false);
    let report = analyze::scan(&targets, &loaded, Some(&base), cache.as_mut());
    render::render_check(&report);

    for file in &report.files {
        if let Some(error) = &file.directive_error {
            eprintln!("directive error: {error}");
        }
    }
    if report.files.iter().any(|f| f.directive_error.is_some()) {
        return Ok(ExitCode::from(EXIT_ERROR));
    }

    let failed = args.fail_on_findings && report.findings_count() > 0;
    Ok(ExitCode::from(if failed {
        EXIT_GATE_FAILED
    } else {
        EXIT_OK
    }))
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

pub fn run_explain(args: ExplainArgs) -> Result<ExitCode, ExitCode> {
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

pub fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn git_error(error: anyhow::Error) -> ExitCode {
    eprintln!("git error: {error}");
    ExitCode::from(EXIT_ERROR)
}
