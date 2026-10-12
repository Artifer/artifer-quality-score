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
use crate::clones;
use crate::detectors::{self, FileContext};
use crate::languages::{spec_for_path, LanguageSpec};
use crate::metrics;
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

impl AnalysisJob {
    /// A job for one file's bytes.
    ///
    /// Every reader of source — the filesystem walk and the git tree alike — builds
    /// its jobs here, so a file is prepared the same way whichever it came from. A
    /// single-file component is reduced to its script block, in place, and may resolve
    /// to a different language than its extension suggested; everything else is taken
    /// as it is.
    pub fn new(path: String, spec: &LanguageSpec, source: Vec<u8>) -> Self {
        if !crate::sfc::is_component(&path) {
            return Self {
                path,
                language: spec.name,
                source,
            };
        }

        let script = crate::sfc::extract(&source);
        Self {
            path,
            language: script.language,
            source: script.source,
        }
    }
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
/// Whether the `&` at `index` is part of an operator (`&&`, `&=`).
///
/// When a parse cascades, the ERROR node can span the whole file, and rewriting `&&` to
/// `&amp;&amp;` would destroy real logical operators — silently lowering complexity and
/// inflating the score.
fn is_operator_ampersand(source: &[u8], index: usize) -> bool {
    let previous = index.checked_sub(1).and_then(|before| source.get(before));
    let next = source.get(index + 1);
    next == Some(&b'&') || previous == Some(&b'&') || next == Some(&b'=')
}

/// Whether the `&` at `index` already begins an entity (`&amp;`, `&#39;`).
fn begins_entity(source: &[u8], index: usize, end: usize) -> bool {
    let tail = &source[index + 1..end];
    tail.first() == Some(&b'#')
        || tail
            .iter()
            .position(|b| !b.is_ascii_alphanumeric())
            .is_some_and(|stop| stop > 0 && tail.get(stop) == Some(&b';'))
}

/// Byte offsets of the bare ampersands inside parser error nodes.
fn bare_ampersand_offsets(source: &[u8], root: &Node) -> Vec<usize> {
    let mut offsets: Vec<usize> = Vec::new();
    for node in metrics::all_nodes(root) {
        if node.kind() != "ERROR" {
            continue;
        }
        let end = node.end_byte().min(source.len());
        for index in node.start_byte()..end {
            let is_bare = source[index] == b'&'
                && !is_operator_ampersand(source, index)
                && !begins_entity(source, index, end);
            if is_bare {
                offsets.push(index);
            }
        }
    }
    offsets
}

fn repair_jsx_ampersands(source: &[u8], root: &Node) -> Option<Vec<u8>> {
    let mut offsets = bare_ampersand_offsets(source, root);
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
    // `parse` only declines when it is cancelled, times out or has no language, and
    // none of those is set up here: broken source still comes back as a tree.
    let tree = parser.parse(source, None).expect("an uncancelled parse");

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
        let retry = parser.parse(&repaired, None).expect("an uncancelled parse");
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

    let ctx = FileContext::new(path.to_string(), spec, profile, root, effective);
    let (raw, suppressed) = detectors::detect(&ctx);
    let mut report = scoring::score_file(&ctx, raw);
    report.syntax_errors = errors;
    report.suppressed = suppressed;
    report.directive_error = ctx.directive_error.clone();
    // Derived from this file alone, and never from its score: the cross-file comparison
    // happens later, in `scan`, so this stays the pure per-file function the cache needs.
    report.fingerprints = clones::fingerprints(&ctx);
    report
}

fn analyze_job(job: &AnalysisJob, profile: &Profile) -> FileReport {
    let spec = crate::languages::spec_for_name(job.language).expect("language was resolved");
    analyze_source(&job.path, &job.source, spec, profile)
}

/// Split a batch into cache hits and the work still to do.
///
/// Returns the result slots, pre-filled where the cache answered, alongside the index
/// and cache key of every job that must actually be analysed.
fn partition_cached(
    jobs: &[AnalysisJob],
    cache: &mut dyn Cache,
    fingerprint: &str,
    enabled: bool,
) -> (Vec<Option<FileReport>>, Vec<(usize, String)>) {
    let mut slots: Vec<Option<FileReport>> = vec![None; jobs.len()];
    let mut pending: Vec<(usize, String)> = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        if !enabled {
            pending.push((index, String::new()));
            continue;
        }
        let key = content_key(&job.path, &job.source, fingerprint);
        match cache.get(&key) {
            Some(hit) => slots[index] = Some(hit),
            None => pending.push((index, key)),
        }
    }
    (slots, pending)
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

    let (mut slots, pending) = partition_cached(jobs, cache, &fingerprint, enabled);

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

/// `path` as the report shows it: relative to `base` when it lies beneath it.
fn display_path(path: &Path, base: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    // A second target outside the first one's directory has no relative form.
    as_posix(resolved.strip_prefix(&base).unwrap_or(path))
}

fn as_posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Whether a file is in a supported language and not excluded.
fn is_scannable(path: &Path, profile: &Profile) -> bool {
    let shown = as_posix(path);
    spec_for_path(&shown).is_some() && !profile.exclude_patterns.matches(&shown)
}

/// Whether `directory` is the root of a linked git worktree.
///
/// A worktree root holds a `.git` *file* rather than a directory, pointing at the real
/// repository: `gitdir: /path/to/repo/.git/worktrees/<name>`. Submodules use the same
/// file form but point into `.git/modules/`, and a submodule is part of the project, so
/// the `worktrees` segment is what distinguishes them.
///
/// Detected rather than matched on a directory name. A project may legitimately have a
/// directory called `trees/` or `worktrees/` full of real source, and being a second
/// checkout of the same repository is a fact that can be read instead of guessed — the
/// same reasoning as `detect_shadcn_ui` reading `components.json`.
fn is_linked_worktree(directory: &Path) -> bool {
    // A `.git` directory, or none at all, reads as nothing — and so as "not a worktree".
    let contents = std::fs::read_to_string(directory.join(".git")).unwrap_or_default();
    let first_line = contents.lines().next().unwrap_or_default();
    let gitdir = first_line.strip_prefix("gitdir:").unwrap_or_default();
    gitdir.replace('\\', "/").contains("/worktrees/")
}

/// Whether to descend into a directory.
///
/// The root is never pruned, so an explicit scan of an excluded directory — or of a
/// worktree — still descends. That is the escape hatch: point `aqs` at a worktree
/// directly and it scores it.
///
/// Excluded directories are pruned purely as a traversal optimisation, since
/// `is_scannable` re-checks every file. Worktree pruning is different: it is the *only*
/// place worktrees are skipped, because nothing in a worktree's path distinguishes it
/// from the original.
fn keep_directory(entry: &walkdir::DirEntry, profile: &Profile) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    if profile.exclude_patterns.matches(&as_posix(entry.path())) {
        return false;
    }
    profile.scan_worktrees || !is_linked_worktree(entry.path())
}

/// Every supported, non-excluded file under `roots`, in sorted order.
///
/// Excluded *directories* are pruned rather than walked and filtered per file. That
/// distinction dominates on a frontend repo: one real checkout held 39,833 files under
/// `node_modules` against 78 source files, so filtering leaves meant pattern-matching
/// 500x more paths than there was source to score.
pub fn discover(roots: &[PathBuf], profile: &Profile) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_scannable(root, profile) {
                found.push(root.clone());
            }
            continue;
        }
        let walker = WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| keep_directory(entry, profile));
        for entry in walker.filter_map(Result::ok) {
            if entry.file_type().is_file() && is_scannable(entry.path(), profile) {
                found.push(entry.path().to_path_buf());
            }
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
    relative_to: &Path,
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
            Ok(source) => jobs.push(AnalysisJob::new(shown, spec, source)),
            Err(error) => unreadable.push(FileReport::unsupported(
                shown,
                spec.name.to_string(),
                error.to_string(),
            )),
        }
    }

    let mut reports = analyze_jobs(&jobs, profile, cache);
    reports.extend(unreadable);

    let scored: Vec<FileReport> = reports
        .iter()
        .filter(|r| r.error.is_none())
        .cloned()
        .collect();
    ScanReport {
        version: VERSION.to_string(),
        profile: profile.name.clone(),
        score: aggregate_score(&scored),
        loc: scored.iter().map(|r| r.loc).sum(),
        // Runs over the finished reports, so cache hits take part without being
        // re-analysed and nothing here can feed back into a file's score.
        observations: clones::cross_file_duplication(&reports),
        files: reports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_that_cannot_be_resolved_is_still_shown_relative_to_its_base() {
        // Nothing here exists, so neither side can be canonicalised.
        let shown = display_path(Path::new("no/such/dir/a.py"), Path::new("no/such"));
        assert_eq!(shown, "dir/a.py");
        let outside = display_path(Path::new("elsewhere/a.py"), Path::new("no/such"));
        assert_eq!(outside, "elsewhere/a.py");
    }
}
