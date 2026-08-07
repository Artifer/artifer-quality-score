//! Changed-file comparison against a git baseline — the fast local loop.
//!
//! `aqs delta` scores only what changed, so it runs in well under a second on any repo
//! size and is cheap enough for a pre-commit hook.
//!
//! Its aggregate covers *the changed files only*. That is the right question for "did I
//! make this worse", but it is **not** the repo score: adding a perfectly reasonable
//! large file can lower a repo's LoC-weighted average without any file regressing. For
//! the "repo score must never go down" gate, use `gate`, which scores both trees in
//! full.
//!
//! Findings are matched between baseline and working tree by (rule, function), never by
//! line number, so inserting a line above a function does not read as a fresh smell.

use crate::analyze::analyze_source;
use crate::cache::Cache;
use crate::gitio::{changed_files, read_blobs, repo_root, resolve_baseline};
use crate::languages::spec_for_path;
use crate::models::{
    aggregate_score, round4, DeltaReport, FileDelta, FileReport, Finding, SCORE_MAX,
};
use crate::profile::Profile;
use crate::VERSION;
use anyhow::Result;
use rayon::prelude::*;
use std::path::Path;

fn finding_key(finding: &Finding) -> (String, String) {
    (
        finding.category.clone(),
        finding.function.clone().unwrap_or_else(|| "<file>".to_string()),
    )
}

fn ordered(mut items: Vec<Finding>) -> Vec<Finding> {
    items.sort_by(|a, b| {
        (a.highlight_range.start_line, &a.category)
            .cmp(&(b.highlight_range.start_line, &b.category))
    });
    items
}

/// Split findings into introduced / fixed / degraded / improved.
pub fn compare_reports(
    old: Option<&FileReport>,
    new: Option<&FileReport>,
) -> (Vec<Finding>, Vec<Finding>, Vec<Finding>, Vec<Finding>) {
    let old_findings: Vec<Finding> =
        old.map(|r| r.all_findings().into_iter().cloned().collect()).unwrap_or_default();
    let new_findings: Vec<Finding> =
        new.map(|r| r.all_findings().into_iter().cloned().collect()).unwrap_or_default();

    let old_keys: Vec<(String, String)> = old_findings.iter().map(finding_key).collect();
    let new_keys: Vec<(String, String)> = new_findings.iter().map(finding_key).collect();

    let mut introduced = Vec::new();
    let mut degraded = Vec::new();
    let mut improved = Vec::new();
    for (index, finding) in new_findings.iter().enumerate() {
        match old_keys.iter().position(|key| *key == new_keys[index]) {
            None => introduced.push(finding.clone()),
            Some(previous) => {
                let before = old_findings[previous].penalty;
                if finding.penalty > before {
                    degraded.push(finding.clone());
                } else if finding.penalty < before {
                    improved.push(finding.clone());
                }
            }
        }
    }
    let fixed: Vec<Finding> = old_findings
        .iter()
        .enumerate()
        .filter(|(index, _)| !new_keys.contains(&old_keys[*index]))
        .map(|(_, finding)| finding.clone())
        .collect();

    (ordered(introduced), ordered(fixed), ordered(degraded), ordered(improved))
}

struct PairJob {
    path: String,
    language: &'static str,
    old_source: Option<Vec<u8>>,
    new_source: Option<Vec<u8>>,
}

fn analyze_pair(
    job: &PairJob,
    profile: &Profile,
) -> (FileDelta, Option<FileReport>, Option<FileReport>) {
    let spec = crate::languages::spec_for_name(job.language).expect("language was resolved");
    let old = job
        .old_source
        .as_ref()
        .map(|source| analyze_source(&job.path, source, spec, profile));
    let new = job
        .new_source
        .as_ref()
        .map(|source| analyze_source(&job.path, source, spec, profile));

    let (introduced, fixed, degraded, improved) = compare_reports(old.as_ref(), new.as_ref());
    let old_score = old.as_ref().map(|r| r.score);
    let new_score = new.as_ref().map(|r| r.score);
    // An absent side counts as a clean 10.0, matching how CodeScene compares against a
    // file that did not exist.
    let change = new_score.unwrap_or(SCORE_MAX) - old_score.unwrap_or(SCORE_MAX);

    let delta = FileDelta {
        path: job.path.clone(),
        old_score,
        new_score,
        score_change: round4(change),
        introduced,
        fixed,
        degraded,
        improved,
    };
    (delta, old, new)
}

/// Score every changed file twice — at the baseline, and as it stands now.
pub fn compute_delta(
    root: &Path,
    profile: &Profile,
    baseline: &str,
    staged: bool,
    merge_base: bool,
    _cache: &mut dyn Cache,
) -> Result<DeltaReport> {
    let repo = repo_root(root)?;
    let resolved = resolve_baseline(&repo, baseline, merge_base)?;
    let targets = changed_files(&repo, &resolved, staged)?;

    let candidates: Vec<(String, &'static str)> = targets
        .into_iter()
        .filter_map(|path| {
            spec_for_path(&path).and_then(|spec| {
                (!profile.is_excluded(&path)).then_some((path, spec.name))
            })
        })
        .collect();

    // One batched git call per side, rather than two subprocesses per file. On a large
    // branch diff that dominates everything else.
    let old_revisions: Vec<String> =
        candidates.iter().map(|(path, _)| format!("{resolved}:{path}")).collect();
    let old_blobs = read_blobs(&repo, &old_revisions)?;
    let staged_blobs = if staged {
        let revisions: Vec<String> =
            candidates.iter().map(|(path, _)| format!(":{path}")).collect();
        read_blobs(&repo, &revisions)?
    } else {
        Vec::new()
    };

    let mut jobs: Vec<PairJob> = Vec::new();
    for (index, (path, language)) in candidates.iter().enumerate() {
        let old_source = old_blobs.get(index).cloned().flatten();
        let new_source = if staged {
            staged_blobs.get(index).cloned().flatten()
        } else {
            let absolute = repo.join(path);
            if absolute.is_file() {
                std::fs::read(&absolute).ok()
            } else {
                None
            }
        };
        if old_source.is_none() && new_source.is_none() {
            continue;
        }
        jobs.push(PairJob {
            path: path.clone(),
            language,
            old_source,
            new_source,
        });
    }

    let results: Vec<(FileDelta, Option<FileReport>, Option<FileReport>)> =
        jobs.par_iter().map(|job| analyze_pair(job, profile)).collect();

    let mut deltas: Vec<FileDelta> = Vec::with_capacity(results.len());
    let mut old_reports: Vec<FileReport> = Vec::new();
    let mut new_reports: Vec<FileReport> = Vec::new();
    for (delta, old, new) in results {
        deltas.push(delta);
        if let Some(report) = old {
            old_reports.push(report);
        }
        if let Some(report) = new {
            new_reports.push(report);
        }
    }

    let label = if merge_base {
        format!("{baseline} (merge-base {})", &resolved[..8.min(resolved.len())])
    } else {
        resolved.clone()
    };
    let old_aggregate = aggregate_score(&old_reports);
    let new_aggregate = aggregate_score(&new_reports);

    deltas.sort_by(|a, b| {
        a.score_change
            .partial_cmp(&b.score_change)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });

    Ok(DeltaReport {
        version: VERSION.to_string(),
        profile: profile.name.clone(),
        baseline: if staged { format!("{label} (staged)") } else { label },
        old_score: old_aggregate,
        new_score: new_aggregate,
        score_change: round4(new_aggregate - old_aggregate),
        files: deltas,
    })
}
