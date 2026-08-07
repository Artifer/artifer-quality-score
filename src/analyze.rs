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
use crate::metrics;
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

/// Rewrite bare `&` inside parser errors as `&amp;`, if that is all that failed.
///
/// A bare `&` in JSX text (`<h1>Werving & Selectie</h1>`, a URL query string) is valid
/// JSX that React renders literally, but tree-sitter-typescript 0.23.2 — the newest
/// published grammar — rejects it. On a real 47k-LoC Next.js codebase this produced
/// spurious errors in 18 files.
///
/// The errors are inert: `&amp;` is semantically identical, and substituting it leaves
/// every score, LoC count and finding unchanged. But an unexplained "scores are
/// approximate" warning on 18 files undermines confidence in a gate, so it is worth
/// removing the false alarm rather than documenting it.
///
/// Only ampersands inside error nodes are touched, and the result is adopted only if
/// it genuinely parses better. Line numbers are preserved, since no newline is added.
fn repair_jsx_ampersands(source: &[u8], root: &Node) -> Option<Vec<u8>> {
    let mut offsets: Vec<usize> = Vec::new();
    for node in metrics::all_nodes(root) {
        if node.kind() != "ERROR" {
            continue;
        }
        let start = node.start_byte();
        let end = node.end_byte().min(source.len());
        let mut index = start;
        while index < end {
            if source[index] != b'&' {
                index += 1;
                continue;
            }
            // Never touch an operator. When a parse cascades, the ERROR node can span
            // the whole file, and rewriting `&&` to `&amp;&amp;` would destroy real
            // logical operators — silently lowering complexity and inflating the score.
            let previous_byte = if index > 0 { source.get(index - 1) } else { None };
            let next_byte = source.get(index + 1);
            let is_operator = next_byte == Some(&b'&')
                || previous_byte == Some(&b'&')
                || next_byte == Some(&b'=');

            // Leave anything that already looks like an entity (`&amp;`, `&#39;`).
            let tail = &source[index + 1..end];
            let is_entity = tail.first() == Some(&b'#')
                || tail
                    .iter()
                    .position(|b| !b.is_ascii_alphanumeric())
                    .map(|stop| stop > 0 && tail.get(stop) == Some(&b';'))
                    .unwrap_or(false);

            if !is_operator && !is_entity {
                offsets.push(index);
            }
            index += 1;
        }
    }
    // ERROR nodes nest, so the same byte can be collected more than once and in
    // tree order rather than byte order. Both would corrupt the rebuild below.
    offsets.sort_unstable();
    offsets.dedup();

    if offsets.is_empty() {
        return None;
    }

    let mut repaired = Vec::with_capacity(source.len() + offsets.len() * 4);
    let mut previous = 0;
    for offset in offsets {
        repaired.extend_from_slice(&source[previous..offset]);
        repaired.extend_from_slice(b"&amp;");
        previous = offset + 1;
    }
    repaired.extend_from_slice(&source[previous..]);
    Some(repaired)
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

    let mut errors = count_syntax_errors(&tree.root_node());
    let mut owned_source: Option<Vec<u8>> = None;
    let mut active = tree;

    // Iterate: one pass only reveals the ampersands inside the *current* error nodes,
    // and a line with several of them recovers one at a time. Bounded so a pathological
    // file cannot loop, and each round must strictly reduce the error count.
    const MAX_REPAIR_ROUNDS: usize = 8;
    for _ in 0..MAX_REPAIR_ROUNDS {
        if errors == 0 {
            break;
        }
        let current = owned_source.as_deref().unwrap_or(source);
        let Some(repaired) = repair_jsx_ampersands(current, &active.root_node()) else {
            break;
        };
        let Some(retry) = parser.parse(&repaired, None) else {
            break;
        };
        let retry_errors = count_syntax_errors(&retry.root_node());
        if retry_errors >= errors {
            break;
        }
        errors = retry_errors;
        active = retry;
        owned_source = Some(repaired);
    }

    let effective = owned_source.as_deref().unwrap_or(source);
    let root = active.root_node();
    let ctx = FileContext::build(path.to_string(), spec, profile, root, effective);
    let raw = detectors::detect(&ctx);
    let mut report = scoring::score_file(&ctx, raw);
    report.syntax_errors = errors;
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
