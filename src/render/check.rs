//! Lint-style output, for an editor or a grep.

use super::*;

/// Lint-style one-line-per-finding output, for editors and grep.
pub fn render_check(report: &ScanReport) {
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

