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
use crate::models::{round4, FileDelta, FileReport, GateReport, ScanReport, SCORE_MAX};
use crate::profile::Profile;
use crate::VERSION;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// Per-file detail for changed files, so a failing gate is actionable.
fn file_deltas(
    baseline: &ScanReport,
    current: &ScanReport,
    changed: &[String],
) -> Vec<FileDelta> {
    let old_by_path: HashMap<&str, &FileReport> =
        baseline.files.iter().map(|r| (r.path.as_str(), r)).collect();
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

/// Compare the whole repository against a baseline revision.
pub fn compute_gate(
    root: &Path,
    profile: &Profile,
    against: &str,
    tolerance: f64,
    merge_base: bool,
    cache: &mut dyn Cache,
) -> Result<GateReport> {
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
    })
}
