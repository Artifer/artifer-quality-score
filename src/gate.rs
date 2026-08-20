//! The merge gate: the repository score must never go down.
//!
//! This scores **both trees in full**, which is the only way to enforce that rule
//! honestly. A changed-files comparison cannot see a regression caused by weight shift:
//! add an 800-line file scoring 8.5 to a repo averaging 9.3 and nothing has regressed,
//! every file is fine, and the repo score still falls. `delta` would report zero; this
//! reports the drop.
//!
//! The baseline is the **merge base** with the target branch by default, so the gate
//! judges only your own changes.

use crate::cache::Cache;
use crate::delta::compare_reports;
use crate::gitio::{changed_files, repo_root, resolve_baseline, scan_revision};
use crate::models::{
    round4, FileDelta, FileReport, GateReport, Observation, ScanReport, SCORE_MAX,
};
use crate::profile::Profile;
use crate::VERSION;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Per-file detail for changed files, so a failing gate is actionable.
fn file_deltas(baseline: &ScanReport, current: &ScanReport, changed: &[String]) -> Vec<FileDelta> {
    let old_by_path: HashMap<&str, &FileReport> = baseline
        .files
        .iter()
        .map(|r| (r.path.as_str(), r))
        .collect();
    let new_by_path: HashMap<&str, &FileReport> =
        current.files.iter().map(|r| (r.path.as_str(), r)).collect();

    let mut sorted_changed: Vec<&String> = changed.iter().collect();
    sorted_changed.sort();

    let mut deltas: Vec<FileDelta> = Vec::new();
    for path in sorted_changed {
        let old = old_by_path.get(path.as_str()).copied();
        let new = new_by_path.get(path.as_str()).copied();
        if old.is_none() && new.is_none() {
            continue;
        }
        let (introduced, fixed, degraded, improved) = compare_reports(old, new);
        let old_score = old.map(|r| r.score);
        let new_score = new.map(|r| r.score);
        let change = new_score.unwrap_or(SCORE_MAX) - old_score.unwrap_or(SCORE_MAX);
        deltas.push(FileDelta {
            path: path.clone(),
            old_score,
            new_score,
            score_change: round4(change),
            introduced,
            fixed,
            degraded,
            improved,
        });
    }

    deltas.sort_by(|a, b| {
        a.score_change
            .partial_cmp(&b.score_change)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
    deltas
}

/// What to gate, and how much slack to allow.
pub struct GateRequest<'a> {
    pub root: &'a Path,
    pub profile: &'a Profile,
    pub against: &'a str,
    pub tolerance: f64,
    pub merge_base: bool,
}

/// Compare the whole repository against a baseline revision.
pub fn compute_gate(request: GateRequest, cache: &mut dyn Cache) -> Result<GateReport> {
    let GateRequest {
        root,
        profile,
        against,
        tolerance,
        merge_base,
    } = request;
    let repo = repo_root(root)?;
    let baseline_revision = resolve_baseline(&repo, against, merge_base)?;

    let baseline = scan_revision(&repo, &baseline_revision, profile, cache)?;
    let current = scan_revision(&repo, "HEAD", profile, cache)?;
    let changed = changed_files(&repo, &baseline_revision, false)?;

    let score_change = round4(current.score - baseline.score);

    Ok(GateReport {
        version: VERSION.to_string(),
        profile: profile.name.clone(),
        against: against.to_string(),
        baseline_revision,
        baseline_score: baseline.score,
        baseline_loc: baseline.loc,
        baseline_file_count: baseline.scored_files().count(),
        current_score: current.score,
        current_loc: current.loc,
        current_file_count: current.scored_files().count(),
        score_change,
        tolerance,
        files: file_deltas(&baseline, &current, &changed),
        observations_introduced: introduced_observations(&baseline, &current),
    })
}

/// Identify an observation by what it is about, not where its lines happen to fall.
///
/// Keyed on category plus the sorted `path::function` of every site: an unrelated edit
/// that shifts a copy down four lines must not make a pre-existing observation look new,
/// or every CI run would report the same duplication as freshly introduced.
fn observation_key(observation: &Observation) -> (String, Vec<String>) {
    let mut sites: Vec<String> = observation
        .sites
        .iter()
        .map(|site| format!("{}::{}", site.path, site.function.as_deref().unwrap_or("")))
        .collect();
    sites.sort();
    (observation.category.clone(), sites)
}

/// Open observations present now that the baseline did not have.
///
/// Dismissed ones are excluded from both sides, so dismissing an observation on a branch
/// registers as resolving it — the same as fixing it, which is the point of the channel.
fn introduced_observations(baseline: &ScanReport, current: &ScanReport) -> Vec<Observation> {
    let before: HashSet<(String, Vec<String>)> = baseline
        .observations
        .iter()
        .filter(|o| o.is_open())
        .map(observation_key)
        .collect();
    current
        .observations
        .iter()
        .filter(|o| o.is_open() && !before.contains(&observation_key(o)))
        .cloned()
        .collect()
}
