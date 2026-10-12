//! Cross-file structural duplication, reported as an observation rather than scored.
//!
//! The intra-file [`crate::detectors::module_level::code_duplication`] rule cannot see
//! this by construction: it compares functions within one file. So the same function
//! copied into six files reads as six clean files, and — worse — *splitting* a file that
//! held two copies moves the duplication from visible to invisible and the score goes up.
//! A metric an LLM optimises against must not have that gradient in it.
//!
//! The fix has to survive two constraints that are not negotiable:
//!
//! **A file's report stays a pure function of its own bytes.** `gate`, `delta` and the
//! content-keyed cache all rest on that. So per-file analysis emits only a
//! [`CloneFingerprint`] per function — still derived from that file alone — and the
//! comparison happens afterwards, in the parent, over fingerprints from every file.
//! A cache hit contributes its fingerprints without being re-parsed, so the join costs
//! a hash lookup per function and no extra passes over the source.
//!
//! **It is not priced.** Attributing a penalty to each participant would make a file's
//! score depend on other files' contents, which breaks the same invariant from the other
//! direction. It is an [`Observation`] instead: visible in every output, resolved by
//! fixing it or by dismissing it with a reason, and never a component of the number.
//!
//! Matching is exact on normalised tokens — identifiers and literals are collapsed, so
//! `confirmAction` and `confirmThing` match, but an 87%-similar pair does not. Fuzzy
//! cross-file matching needs an index that costs more than the whole scan currently does.

use crate::detectors::FileContext;
use crate::metrics;
use crate::models::{CloneFingerprint, FileReport, Observation, ObservationSite};
use crate::profile::CROSS_FILE_DUPLICATION;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Structural hash of one function's normalised token sequence.
///
/// Truncated to 64 bits: across a repository of even 10^5 functions the collision odds
/// are around 10^-9, and a full digest would treble the size of a cached report for no
/// reachable benefit.
fn structural_hash(tokens: &[&'static str]) -> String {
    let mut hasher = Sha256::new();
    for token in tokens {
        hasher.update(token.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// Fingerprint every function in a file that is big enough to be worth matching.
///
/// Test files are skipped. Suites repeat their setup on purpose, and the test-side
/// duplication question already has its own rule in
/// [`crate::detectors::test_level::duplicated_assertion_blocks`]; flagging every shared
/// `renderComponent` helper across a suite would bury the production signal.
pub fn fingerprints(ctx: &FileContext) -> Vec<CloneFingerprint> {
    if ctx.is_test {
        return Vec::new();
    }
    let min_loc = ctx.threshold("cross_file_duplication_min_lines_of_code");

    let mut found = Vec::new();
    for (index, function) in ctx.functions.iter().enumerate() {
        // Too small to be news, or nothing to compare: every empty token sequence would
        // hash alike.
        let tokens = if (function.loc as f64) >= min_loc {
            metrics::normalised_tokens(&function.node, ctx.spec, ctx.source)
        } else {
            Vec::new()
        };
        if tokens.is_empty() {
            continue;
        }
        // Resolved here because this is the only place the file's directives are in
        // scope; the join runs over reports, long after the source is gone.
        let dismissed = ctx
            .suppression(Some(index), CROSS_FILE_DUPLICATION)
            .map(|directive| directive.reason.clone());
        found.push(CloneFingerprint {
            hash: structural_hash(&tokens),
            function: function.name.clone(),
            line: function.range.start_line,
            loc: function.loc,
            dismissed,
        });
    }
    found
}

/// One group of structurally identical functions, before it becomes an observation.
struct Group {
    sites: Vec<ObservationSite>,
    paths: Vec<String>,
    loc: usize,
    dismissed: Option<String>,
}

/// Bucket every fingerprint in the scan by its structural hash.
fn group_by_hash(files: &[FileReport]) -> HashMap<&str, Group> {
    let mut groups: HashMap<&str, Group> = HashMap::new();
    for file in files.iter().filter(|f| f.error.is_none()) {
        for fingerprint in &file.fingerprints {
            let group = groups
                .entry(fingerprint.hash.as_str())
                .or_insert_with(|| Group {
                    sites: Vec::new(),
                    paths: Vec::new(),
                    loc: fingerprint.loc,
                    dismissed: None,
                });
            group.sites.push(ObservationSite {
                path: file.path.clone(),
                line: fingerprint.line,
                function: Some(fingerprint.function.clone()),
            });
            if !group.paths.contains(&file.path) {
                group.paths.push(file.path.clone());
            }
            // One directive anywhere in the group settles it: the author of any copy is
            // in a position to say the repetition is deliberate, and requiring the same
            // comment in all six files would be its own kind of duplication.
            if group.dismissed.is_none() {
                group.dismissed.clone_from(&fingerprint.dismissed);
            }
        }
    }
    groups
}

/// Every set of structurally identical functions that spans more than one file.
///
/// Groups confined to a single file are left alone: that is the scored intra-file rule's
/// territory, and reporting it twice would charge for it twice in the reader's head.
pub fn cross_file_duplication(files: &[FileReport]) -> Vec<Observation> {
    let mut observations: Vec<Observation> = Vec::new();

    for group in group_by_hash(files).into_values() {
        if group.paths.len() < 2 {
            continue;
        }
        let mut sites = group.sites;
        sites.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        observations.push(Observation {
            category: CROSS_FILE_DUPLICATION.to_string(),
            details: format!(
                "{} copies across {} files, {} LoC each",
                sites.len(),
                group.paths.len(),
                group.loc
            ),
            sites,
            dismissed: group.dismissed,
        });
    }

    // HashMap order is not deterministic and this output is compared between runs, so
    // the sort is load-bearing rather than cosmetic. Worst first, matching every other
    // report in the tool: most copies, then widest spread, then source order.
    observations.sort_by(|a, b| {
        b.sites.len().cmp(&a.sites.len()).then_with(|| {
            (&a.sites[0].path, a.sites[0].line).cmp(&(&b.sites[0].path, b.sites[0].line))
        })
    });
    observations
}
