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
        finding
            .function
            .clone()
            .unwrap_or_else(|| "<file>".to_string()),
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
    let old_findings: Vec<Finding> = old
        .map(|r| r.all_findings().into_iter().cloned().collect())
        .unwrap_or_default();
    let new_findings: Vec<Finding> = new
        .map(|r| r.all_findings().into_iter().cloned().collect())
        .unwrap_or_default();

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

    (
        ordered(introduced),
        ordered(fixed),
        ordered(degraded),
        ordered(improved),
    )
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

/// What to compare, and how. Six loose parameters at the call site is the smell this
/// tool reports as Excess Number of Function Arguments.
pub struct DeltaRequest<'a> {
    pub root: &'a Path,
    pub profile: &'a Profile,
    pub baseline: &'a str,
    pub staged: bool,
    pub merge_base: bool,
}

/// Collect the baseline and current bytes for every changed file.
fn pair_jobs(
    repo: &Path,
    resolved: &str,
    request: &DeltaRequest,
    targets: Vec<String>,
) -> Result<Vec<PairJob>> {
    let profile = request.profile;
    let candidates: Vec<(String, &'static str)> = targets
        .into_iter()
        .filter_map(|path| {
            spec_for_path(&path).and_then(|spec| {
                (!profile.exclude_patterns.matches(&path)).then_some((path, spec.name))
            })
        })
        .collect();

    // One batched git call per side, rather than two subprocesses per file. On a large
    // branch diff that dominates everything else.
    let old_revisions: Vec<String> = candidates
        .iter()
        .map(|(path, _)| format!("{resolved}:{path}"))
        .collect();
    let old_blobs = read_blobs(repo, &old_revisions)?;
    let staged_blobs = if request.staged {
        let revisions: Vec<String> = candidates
            .iter()
            .map(|(path, _)| format!(":{path}"))
            .collect();
        read_blobs(repo, &revisions)?
    } else {
        Vec::new()
    };

    let mut jobs: Vec<PairJob> = Vec::new();
    for (index, (path, language)) in candidates.iter().enumerate() {
        let old_source = old_blobs.get(index).cloned().flatten();
        let new_source = if request.staged {
            staged_blobs.get(index).cloned().flatten()
        } else {
            let absolute = repo.join(path);
            absolute
                .is_file()
                .then(|| std::fs::read(&absolute).ok())
                .flatten()
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
    Ok(jobs)
}

/// How the report names what it compared against.
fn baseline_label(request: &DeltaRequest, resolved: &str) -> String {
    let label = if request.merge_base {
        format!(
            "{} (merge-base {})",
            request.baseline,
            &resolved[..8.min(resolved.len())]
        )
    } else {
        resolved.to_string()
    };
    if request.staged {
        format!("{label} (staged)")
    } else {
        label
    }
}

/// Worst regression first, then by path so equal changes keep a stable order.
fn worst_first(deltas: &mut [FileDelta]) {
    deltas.sort_by(|a, b| {
        a.score_change
            .partial_cmp(&b.score_change)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.path.cmp(&b.path))
    });
}

/// Score every changed file twice — at the baseline, and as it stands now.
pub fn compute_delta(request: DeltaRequest, _cache: &mut dyn Cache) -> Result<DeltaReport> {
    let repo = repo_root(request.root)?;
    let resolved = resolve_baseline(&repo, request.baseline, request.merge_base)?;
    let targets = changed_files(&repo, &resolved, request.staged)?;
    let jobs = pair_jobs(&repo, &resolved, &request, targets)?;

    let results: Vec<(FileDelta, Option<FileReport>, Option<FileReport>)> = jobs
        .par_iter()
        .map(|job| analyze_pair(job, request.profile))
        .collect();

    let mut deltas: Vec<FileDelta> = Vec::with_capacity(results.len());
    let mut old_reports: Vec<FileReport> = Vec::new();
    let mut new_reports: Vec<FileReport> = Vec::new();
    for (delta, old, new) in results {
        deltas.push(delta);
        old_reports.extend(old);
        new_reports.extend(new);
    }
    worst_first(&mut deltas);

    let old_score = aggregate_score(&old_reports);
    let new_score = aggregate_score(&new_reports);
    Ok(DeltaReport {
        version: VERSION.to_string(),
        profile: request.profile.name.clone(),
        baseline: baseline_label(&request, &resolved),
        old_score,
        new_score,
        score_change: round4(new_score - old_score),
        files: deltas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Range;

    fn finding(category: &str, function: Option<&str>, penalty: f64) -> Finding {
        Finding {
            category: category.to_string(),
            details: String::new(),
            highlight_range: Range::line(1),
            penalty,
            value: None,
            threshold: None,
            function: function.map(str::to_string),
        }
    }

    fn report(findings: Vec<Finding>) -> FileReport {
        let mut report = FileReport::unsupported("a.py".into(), "python".into(), String::new());
        report.file_level_code_smells = findings;
        report
    }

    fn names(findings: &[Finding]) -> Vec<(&str, Option<&str>)> {
        findings
            .iter()
            .map(|f| (f.category.as_str(), f.function.as_deref()))
            .collect()
    }

    #[test]
    fn a_finding_is_matched_on_both_its_rule_and_its_function() {
        // Same rule in another function, and another rule in the same function: neither
        // is the old finding come back, so one is fixed and two are introduced.
        let old = report(vec![finding("Large Method", Some("load"), 1.0)]);
        let new = report(vec![
            finding("Large Method", Some("save"), 1.0),
            finding("Complex Method", Some("load"), 1.0),
        ]);
        let (introduced, fixed, degraded, improved) = compare_reports(Some(&old), Some(&new));
        assert_eq!(
            names(&introduced),
            [
                ("Complex Method", Some("load")),
                ("Large Method", Some("save"))
            ]
        );
        assert_eq!(names(&fixed), [("Large Method", Some("load"))]);
        assert!(degraded.is_empty() && improved.is_empty());
    }

    #[test]
    fn a_file_level_finding_is_not_a_function_called_nothing() {
        let old = report(vec![finding("Large Method", None, 1.0)]);
        let new = report(vec![finding("Large Method", Some(""), 1.0)]);
        let (introduced, fixed, _, _) = compare_reports(Some(&old), Some(&new));
        assert_eq!(names(&introduced), [("Large Method", Some(""))]);
        assert_eq!(names(&fixed), [("Large Method", None)]);
    }

    #[test]
    fn the_worst_regression_comes_first_and_ties_go_by_path() {
        let delta = |path: &str, score_change: f64| FileDelta {
            path: path.to_string(),
            old_score: None,
            new_score: None,
            score_change,
            introduced: Vec::new(),
            fixed: Vec::new(),
            degraded: Vec::new(),
            improved: Vec::new(),
        };
        let mut deltas = [delta("b.py", 0.5), delta("c.py", -1.0), delta("a.py", 0.5)];
        worst_first(&mut deltas);
        let order: Vec<&str> = deltas.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(order, ["c.py", "a.py", "b.py"]);
    }

    #[test]
    fn a_finding_that_costs_the_same_as_before_is_not_reported() {
        let same = report(vec![finding("Large Method", Some("load"), 1.0)]);
        let (introduced, fixed, degraded, improved) = compare_reports(Some(&same), Some(&same));
        assert!(introduced.is_empty() && fixed.is_empty());
        assert!(degraded.is_empty() && improved.is_empty());
    }
}
