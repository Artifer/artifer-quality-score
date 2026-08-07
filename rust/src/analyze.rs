//! File discovery and the analysis pipeline.
//!
//! ```text
//! bytes -> tree-sitter tree -> FileContext -> findings -> priced findings -> score
//! ```
//!
//! The pipeline is a pure function of (file bytes, profile, tool version). No git
//! history, no network, no clock, no filesystem-order dependence — which is what lets
//! this be used as a regression gate at all, and what makes the report cache sound.

use crate::cache::{content_key, Cache};
use crate::detectors::{self, FileContext};
use crate::languages::{spec_for_path, LanguageSpec};
use crate::models::{aggregate_score, FileReport, ScanReport};
use crate::profile::Profile;
use crate::scoring;
use crate::VERSION;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};
use walkdir::WalkDir;

/// One analysis unit: display path, language name, and source bytes.
pub struct AnalysisJob {
    pub path: String,
    pub language: &'static str,
    pub source: Vec<u8>,
}

fn count_syntax_errors(root: &Node) -> usize {
    if !root.has_error() {
        return 0;
    }
    let mut errors = 0;
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        if node.kind() == "ERROR" || node.is_missing() {
            errors += 1;
            continue;
        }
        if node.has_error() {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }
    errors
}

/// Score one already-loaded source file.
pub fn analyze_source(
    path: &str,
    source: &[u8],
    spec: &'static LanguageSpec,
    profile: &Profile,
) -> FileReport {
    let mut parser = Parser::new();
    parser
        .set_language(&spec.language())
        .expect("pinned grammar is compatible");
    let Some(tree) = parser.parse(source, None) else {
        return FileReport::unsupported(
            path.to_string(),
            spec.name.to_string(),
            "could not parse".to_string(),
        );
    };

    let root = tree.root_node();
    let ctx = FileContext::build(path.to_string(), spec, profile, root, source);
    let raw = detectors::detect(&ctx);
    let mut report = scoring::score_file(&ctx, raw);
    report.syntax_errors = count_syntax_errors(&root);
    report
}

fn analyze_job(job: &AnalysisJob, profile: &Profile) -> FileReport {
    let spec = crate::languages::spec_for_name(job.language).expect("language was resolved");
    analyze_source(&job.path, &job.source, spec, profile)
}

/// Score a batch of jobs, serving what it can from `cache`.
///
/// Cache lookups and writes happen here, in the parent, so the parallel workers stay
/// pure functions of their input. Output order matches input order, so parallelism is
/// never observable in the report.
pub fn analyze_jobs(
    jobs: &[AnalysisJob],
    profile: &Profile,
    cache: &mut dyn Cache,
) -> Vec<FileReport> {
    let fingerprint = cache.fingerprint().to_string();
    let enabled = cache.enabled();

    let mut slots: Vec<Option<FileReport>> = vec![None; jobs.len()];
    let mut pending: Vec<(usize, String)> = Vec::new();

    for (index, job) in jobs.iter().enumerate() {
        if !enabled {
            pending.push((index, String::new()));
            continue;
        }
        let key = content_key(&job.path, &job.source, &fingerprint);
        match cache.get(&key) {
            Some(hit) => slots[index] = Some(hit),
            None => pending.push((index, key)),
        }
    }

    let computed: Vec<(usize, String, FileReport)> = pending
        .into_par_iter()
        .map(|(index, key)| {
            let report = analyze_job(&jobs[index], profile);
            (index, key, report)
        })
        .collect();

    for (index, key, report) in computed {
        if enabled {
            cache.put(&key, &report);
        }
        slots[index] = Some(report);
    }
    cache.commit();

    slots.into_iter().flatten().collect()
}

fn display_path(path: &Path, relative_to: Option<&Path>) -> String {
    let normalise = |p: &Path| p.to_string_lossy().replace('\\', "/");
    match relative_to {
        None => normalise(path),
        Some(base) => {
            let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
            match resolved.strip_prefix(&base) {
                Ok(relative) => normalise(relative),
                Err(_) => normalise(path),
            }
        }
    }
}

/// Every supported, non-excluded file under `roots`, in sorted order.
pub fn discover(roots: &[PathBuf], profile: &Profile) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        if root.is_file() {
            let shown = root.to_string_lossy().replace('\\', "/");
            if spec_for_path(&shown).is_some() && !profile.is_excluded(&shown) {
                found.push(root.clone());
            }
            continue;
        }
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }
            let shown = entry.path().to_string_lossy().replace('\\', "/");
            if spec_for_path(&shown).is_none() || profile.is_excluded(&shown) {
                continue;
            }
            found.push(entry.path().to_path_buf());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Score every supported file under `roots`.
pub fn scan(
    roots: &[PathBuf],
    profile: &Profile,
    relative_to: Option<&Path>,
    cache: &mut dyn Cache,
) -> ScanReport {
    let paths = discover(roots, profile);

    let mut unreadable: Vec<FileReport> = Vec::new();
    let mut jobs: Vec<AnalysisJob> = Vec::new();
    for path in &paths {
        let shown = display_path(path, relative_to);
        let spec = spec_for_path(&path.to_string_lossy().replace('\\', "/"))
            .expect("discover filtered on this");
        match std::fs::read(path) {
            Ok(source) => jobs.push(AnalysisJob {
                path: shown,
                language: spec.name,
                source,
            }),
            Err(error) => unreadable.push(FileReport::unsupported(
                shown,
                spec.name.to_string(),
                error.to_string(),
            )),
        }
    }

    let mut reports = analyze_jobs(&jobs, profile, cache);
    reports.extend(unreadable);

    let scored: Vec<FileReport> =
        reports.iter().filter(|r| r.error.is_none()).cloned().collect();
    ScanReport {
        version: VERSION.to_string(),
        profile: profile.name.clone(),
        score: aggregate_score(&scored),
        loc: scored.iter().map(|r| r.loc).sum(),
        files: reports,
    }
}
