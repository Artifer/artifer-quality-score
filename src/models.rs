//! Report data model.
//!
//! Field names and nesting match the Python reference implementation exactly, so
//! the two produce interchangeable JSON. That is what makes differential testing
//! possible: any disagreement is a bug, measured against a known-good oracle.

use serde::{Deserialize, Serialize};

pub const SCORE_MAX: f64 = 10.0;
pub const SCORE_MIN: f64 = 1.0;

/// Round to 4 decimal places the way Python's `round(x, 4)` does.
///
/// Ties go to even, not away from zero. Every score and penalty crossing the JSON
/// boundary passes through here, so a mismatch would show up as a diff against the
/// reference implementation on the very first file.
pub fn round4(value: f64) -> f64 {
    (value * 10_000.0).round_ties_even() / 10_000.0
}

/// 1-indexed, inclusive source range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl Range {
    pub fn line(line: usize) -> Self {
        Self {
            start_line: line,
            start_column: 1,
            end_line: line,
            end_column: 1,
        }
    }
}

/// One code smell at one location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub category: String,
    /// Human-readable measurement, e.g. "cc = 14".
    pub details: String,
    pub highlight_range: Range,
    /// Points deducted from this file's score, after rule weighting.
    pub penalty: f64,
    /// The measured value the rule fired on, for machine consumers.
    pub value: Option<f64>,
    /// The configured threshold it exceeded.
    pub threshold: Option<f64>,
    /// Enclosing function, when the smell is function- or expression-level.
    pub function: Option<String>,
}

/// A finding an author deliberately turned off with a source comment.
///
/// Recorded rather than dropped: a suppression is a decision, and a reviewer should be
/// able to see what a branch chose to ignore and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Suppression {
    pub category: String,
    pub details: String,
    pub highlight_range: Range,
    pub function: Option<String>,
    /// The justification from the directive, which is mandatory.
    pub reason: String,
    /// 1-indexed line the directive itself appears on.
    pub directive_line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionReport {
    pub function: String,
    pub range: Range,
    pub code_smells: Vec<Finding>,
}

/// The structural signature of one function, for cross-file matching.
///
/// Computed from the file's own bytes and nothing else, which is what lets it ride along
/// in the content-keyed cache: a cache hit contributes its fingerprints to the cross-file
/// join without the file being re-parsed.
///
/// `hash` covers the normalised token sequence, so two functions that differ only in
/// identifiers and literals share a fingerprint. Matching is exact rather than fuzzy —
/// a hash join has no notion of "87% similar" — which trades recall for a join that costs
/// nothing and never guesses. The intra-file [`Finding`] rule stays fuzzy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloneFingerprint {
    pub hash: String,
    pub function: String,
    pub line: usize,
    pub loc: usize,
    /// The reason from an `@aqs` directive on this function, when one dismisses
    /// cross-file duplication here. Resolved during per-file analysis, because that is
    /// the only place the directives are in scope.
    pub dismissed: Option<String>,
}

/// One site participating in an observation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationSite {
    pub path: String,
    pub line: usize,
    pub function: Option<String>,
}

/// Something true about the code that deliberately carries no penalty.
///
/// The score answers "how hard is this file to read", and every point it deducts traces
/// to one measurement inside one file. Some real problems do not fit that shape: they
/// span files, or they depend on intent the tool cannot read. Pricing them would either
/// make a file's score depend on other files — breaking the invariant `gate`, `delta` and
/// the cache all rest on — or charge for a judgement the tool is not entitled to make.
///
/// So they are reported instead, and resolved the same way a finding is: fix it, or
/// dismiss it with an `@aqs` directive whose reason stays visible in the report. An
/// observation list that reaches empty is the stopping condition, in place of a number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub category: String,
    pub details: String,
    /// Every place the observation is about, in path order. Always at least two — a
    /// one-site observation is a finding, and belongs in the score.
    pub sites: Vec<ObservationSite>,
    /// The author's justification, when an `@aqs` directive at one of the sites
    /// dismissed it. Recorded rather than dropped, exactly as suppressions are.
    pub dismissed: Option<String>,
}

impl Observation {
    pub fn is_open(&self) -> bool {
        self.dismissed.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String,
    pub language: String,
    /// Non-blank, non-comment lines.
    pub loc: usize,
    pub score: f64,
    pub file_level_code_smells: Vec<Finding>,
    pub function_level_code_smells: Vec<FunctionReport>,
    /// Set when the file could not be read or has no supported language, in which
    /// case it is excluded from aggregation entirely.
    pub error: Option<String>,
    /// Count of syntax errors tree-sitter recovered from. The file is still scored,
    /// but a non-zero count means treat the number with suspicion.
    pub syntax_errors: usize,
    /// Findings turned off by an `@aqs(...)` comment. Excluded from the score, but
    /// listed so that suppression is visible rather than silent.
    #[serde(default)]
    pub suppressed: Vec<Suppression>,
    /// A malformed `@aqs(...)` directive in this file.
    ///
    /// The file is still scored, deliberately as though the directive were absent, so a
    /// broken suppression can never *lower* a score. The CLI exits non-zero and prints
    /// this, because a directive that does not work is worse than no directive.
    #[serde(default)]
    pub directive_error: Option<String>,
    /// Structural signatures of this file's functions, for the cross-file join.
    ///
    /// Deliberately **not** `#[serde(default)]`. A cache entry written before this field
    /// existed must fail to deserialise, which `sqlite::get` already treats as a miss and
    /// recomputes. Defaulting it would instead resurrect the entry with no fingerprints,
    /// and the file would drop silently out of every cross-file observation — a wrong
    /// answer that looks exactly like a clean one.
    pub fingerprints: Vec<CloneFingerprint>,
}

impl FileReport {
    pub fn unsupported(path: String, language: String, error: String) -> Self {
        Self {
            path,
            language,
            loc: 0,
            score: SCORE_MAX,
            file_level_code_smells: Vec::new(),
            function_level_code_smells: Vec::new(),
            error: Some(error),
            syntax_errors: 0,
            suppressed: Vec::new(),
            directive_error: None,
            fingerprints: Vec::new(),
        }
    }

    pub fn all_findings(&self) -> Vec<&Finding> {
        let mut findings: Vec<&Finding> = self.file_level_code_smells.iter().collect();
        for group in &self.function_level_code_smells {
            findings.extend(group.code_smells.iter());
        }
        findings
    }

    pub fn finding_count(&self) -> usize {
        self.file_level_code_smells.len()
            + self
                .function_level_code_smells
                .iter()
                .map(|g| g.code_smells.len())
                .sum::<usize>()
    }
}

// @aqs-file(disable:"Low Cohesion") report structs are data, and LCOM is a class-design
// signal — ScanReport carries per-file results and cross-file results, and its accessors
// group by that on purpose. Merging them to connect the groups would be metric-gaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    /// Tool version, so a score can always be traced to the code that produced it.
    pub version: String,
    /// Name of the scoring profile in force.
    pub profile: String,
    /// LoC-weighted mean of file scores.
    pub score: f64,
    pub loc: usize,
    pub files: Vec<FileReport>,
    /// Cross-file facts that carry no penalty. See [`Observation`].
    #[serde(default)]
    pub observations: Vec<Observation>,
}

impl ScanReport {
    pub fn scored_files(&self) -> impl Iterator<Item = &FileReport> {
        self.files.iter().filter(|f| f.error.is_none())
    }

    pub fn findings_count(&self) -> usize {
        self.files.iter().map(|f| f.finding_count()).sum()
    }

    /// Observations nobody has fixed or dismissed yet — the checklist that has to
    /// reach zero, alongside the score that has to reach its target.
    pub fn open_observations(&self) -> usize {
        self.observations.iter().filter(|o| o.is_open()).count()
    }
}

/// Score change for a single file between a baseline and the working tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDelta {
    pub path: String,
    pub old_score: Option<f64>,
    pub new_score: Option<f64>,
    pub score_change: f64,
    /// Smells present now but not in the baseline.
    pub introduced: Vec<Finding>,
    /// Smells present in the baseline but not now.
    pub fixed: Vec<Finding>,
    /// Smells present in both, but measurably worse now.
    pub degraded: Vec<Finding>,
    /// Smells present in both, but measurably better now.
    pub improved: Vec<Finding>,
}

impl FileDelta {
    pub fn is_regression(&self) -> bool {
        self.score_change < 0.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaReport {
    pub version: String,
    pub profile: String,
    pub baseline: String,
    pub old_score: f64,
    pub new_score: f64,
    pub score_change: f64,
    pub files: Vec<FileDelta>,
}

/// Whole-repository comparison between a baseline revision and now.
///
/// Distinct from `DeltaReport`: these scores are LoC-weighted over *every* file in
/// each tree, not just the changed ones. That is what lets it catch a regression
/// caused purely by weight shift — a new large file that is individually fine but
/// below the repository's average.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateReport {
    pub version: String,
    pub profile: String,
    /// The ref the user asked to compare against, e.g. "origin/develop".
    pub against: String,
    /// The commit actually used — the merge base, unless disabled.
    pub baseline_revision: String,
    pub baseline_score: f64,
    pub baseline_loc: usize,
    pub baseline_file_count: usize,
    pub current_score: f64,
    pub current_loc: usize,
    pub current_file_count: usize,
    pub score_change: f64,
    /// Allowed drop before the gate fails.
    pub tolerance: f64,
    /// Per-file detail for the changed files, so a failure is actionable.
    pub files: Vec<FileDelta>,
    /// Observations present now that the baseline did not have.
    ///
    /// Reported, never gated on: [`Observation`] is unscored by construction, and a gate
    /// that failed on one would be pricing it through the back door. It is here because
    /// a channel nobody sees in CI is a channel that does not exist — a branch that
    /// copies a function into a second file passes cleanly on score alone, and this is
    /// the line that says so.
    #[serde(default)]
    pub observations_introduced: Vec<Observation>,
}

impl GateReport {
    pub fn passed(&self) -> bool {
        self.score_change >= -self.tolerance.abs()
    }
}

/// LoC-weighted mean, so a 3000-line mess is not offset by ten clean stubs.
pub fn aggregate_score(files: &[FileReport]) -> f64 {
    let scored: Vec<&FileReport> = files
        .iter()
        .filter(|f| f.error.is_none() && f.loc > 0)
        .collect();
    if scored.is_empty() {
        return SCORE_MAX;
    }
    let weighted: f64 = scored.iter().map(|f| f.score * f.loc as f64).sum();
    let total_loc: f64 = scored.iter().map(|f| f.loc as f64).sum();
    round4(weighted / total_loc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_ties_to_even_like_python() {
        assert_eq!(round4(0.123_45), 0.123_4);
        assert_eq!(round4(0.123_55), 0.123_6);
        assert_eq!(round4(1.0), 1.0);
    }

    fn report(score: f64, loc: usize) -> FileReport {
        FileReport {
            path: "f.py".into(),
            language: "python".into(),
            loc,
            score,
            file_level_code_smells: vec![],
            function_level_code_smells: vec![],
            error: None,
            syntax_errors: 0,
            suppressed: vec![],
            directive_error: None,
            fingerprints: vec![],
        }
    }

    #[test]
    fn weights_by_lines_of_code() {
        // CodeScene's published example: the naive mean would be 7.33.
        let files = vec![report(2.0, 5000), report(10.0, 100), report(10.0, 10)];
        assert_eq!(aggregate_score(&files), round4(11_100.0 / 5_110.0));
        assert!(aggregate_score(&files) < 2.2);
    }

    #[test]
    fn empty_input_is_perfect_not_zero() {
        assert_eq!(aggregate_score(&[]), SCORE_MAX);
    }

    #[test]
    fn a_file_with_no_code_or_no_reading_carries_no_weight() {
        let empty = report(4.0, 0);
        assert_eq!(aggregate_score(std::slice::from_ref(&empty)), SCORE_MAX);
        let mut unread = report(2.0, 300);
        unread.error = Some("unreadable".into());
        assert_eq!(aggregate_score(&[unread, empty, report(9.0, 10)]), 9.0);
    }

    #[test]
    fn only_a_falling_score_is_a_regression() {
        let moved = |score_change: f64| FileDelta {
            path: "f.py".into(),
            old_score: None,
            new_score: None,
            score_change,
            introduced: vec![],
            fixed: vec![],
            degraded: vec![],
            improved: vec![],
        };
        assert!(moved(-0.01).is_regression());
        assert!(!moved(0.0).is_regression());
        assert!(!moved(0.01).is_regression());
    }
}
