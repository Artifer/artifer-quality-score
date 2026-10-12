//! What the behavioural suites share: score a snippet, then ask what was found.

use aqs::analyze::analyze_source;
use aqs::languages::spec_for_name;
use aqs::models::FileReport;
use aqs::profile::{self as prof, Profile};
use std::collections::HashSet;

pub fn score(source: &str, path: &str, language: &str) -> FileReport {
    score_with(&Profile::default(), source, path, language)
}

/// Score under a profile other than the default, for a threshold a project changed.
pub fn score_with(profile: &Profile, source: &str, path: &str, language: &str) -> FileReport {
    let spec = spec_for_name(language).expect("known language");
    analyze_source(path, source.as_bytes(), spec, profile)
}

pub fn categories(report: &FileReport) -> HashSet<String> {
    report
        .all_findings()
        .iter()
        .map(|f| f.category.clone())
        .collect()
}

pub fn detail(report: &FileReport, category: &str) -> Option<String> {
    report
        .all_findings()
        .iter()
        .find(|f| f.category == category)
        .map(|f| f.details.clone())
}

/// A named closure nested in `outer` is measured as its own unit: the complexity is
/// charged to `inner` and to nothing else.
pub fn assert_only_inner_is_complex(report: &FileReport) {
    let flagged: HashSet<String> = report
        .all_findings()
        .iter()
        .filter(|f| f.category == prof::COMPLEX_METHOD)
        .map(|f| f.function.clone().unwrap_or_default())
        .collect();
    assert_eq!(flagged, HashSet::from(["inner".to_string()]));
}

pub fn py(source: &str) -> FileReport {
    score(source, "sample.py", "python")
}

/// Build a throwaway tree and return its root. Caller removes it.
pub fn scratch_tree(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("aqs-test-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    for (relative, body) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }
    root
}

/// The class holds together: whatever else was found, Low Cohesion was not.
pub fn assert_cohesive(report: &FileReport) {
    let details: Vec<&String> = report.all_findings().iter().map(|f| &f.details).collect();
    assert!(
        !categories(report).contains(prof::LOW_COHESION),
        "{details:?}"
    );
}
