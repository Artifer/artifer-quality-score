//! Lint-style output, for an editor or a grep.

use super::*;

/// Open observations, one line per site.
///
/// Same shape as a finding, with `(observation)` where the penalty would be. An agent
/// reading this output has to be able to tell the two apart without being able to
/// overlook either, so they share a format and differ in the one field that matters.
fn render_observations(report: &ScanReport) {
    for observation in report.observations.iter().filter(|o| o.is_open()) {
        for site in &observation.sites {
            let where_ = site
                .function
                .as_deref()
                .map(|name| format!(" [{name}]"))
                .unwrap_or_default();
            println!(
                "{}:{}:1: {}: {}{} (observation)",
                site.path, site.line, observation.category, observation.details, where_
            );
        }
    }
}

/// Lint-style one-line-per-finding output, for editors and grep.
pub fn render_check(report: &ScanReport) {
    render_observations(report);
    for file in &report.files {
        let mut findings = file.all_findings();
        findings.sort_by(|a, b| {
            (a.highlight_range.start_line, &a.category)
                .cmp(&(b.highlight_range.start_line, &b.category))
        });
        for finding in findings {
            let where_ = finding
                .function
                .as_deref()
                .map(|name| format!(" [{name}]"))
                .unwrap_or_default();
            println!(
                "{}:{}:{}: {}: {}{} (-{:.2})",
                file.path,
                finding.highlight_range.start_line,
                finding.highlight_range.start_column,
                finding.category,
                finding.details,
                where_,
                finding.penalty
            );
        }
    }
}
