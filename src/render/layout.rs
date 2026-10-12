//! Column widths, computed by hand.
//!
//! Not handed to a table library: those are free to collapse a fixed-width column under
//! pressure, and the line number and penalty are the two things that must never
//! disappear when the terminal is narrow.

use super::*;

pub(crate) struct Layout {
    pub(crate) rule: usize,
    pub(crate) function: usize,
    pub(crate) details: usize,
    pub(crate) show_function: bool,
}

pub(crate) const LINE_WIDTH: usize = 4;

pub(crate) const PENALTY_WIDTH: usize = 6;

pub(crate) const INDENT: usize = 3;

pub(crate) const GAP: usize = 2;

impl Layout {
    pub(crate) fn for_width(width: usize) -> Self {
        let fixed = INDENT + LINE_WIDTH + PENALTY_WIDTH + GAP * 4;
        let flexible = width.saturating_sub(fixed).max(24);
        if flexible < 46 {
            let rule = ((flexible as f64 * 0.45) as usize).max(14);
            return Self {
                rule,
                function: 0,
                details: flexible.saturating_sub(rule).max(10),
                show_function: false,
            };
        }
        let rule = ((flexible as f64 * 0.36) as usize).clamp(14, 34);
        let function = ((flexible as f64 * 0.26) as usize).clamp(8, 24);
        Self {
            rule,
            function,
            details: flexible.saturating_sub(rule + function + GAP).max(10),
            show_function: true,
        }
    }
}

pub(crate) fn penalty_text(finding: &Finding, style: &Style) -> String {
    if finding.penalty > 0.0 {
        style.paint(
            &format!("{:>PENALTY_WIDTH$}", format!("-{:.2}", finding.penalty)),
            RED,
        )
    } else {
        // Capped out: the finding is real, but this rule has already spent its
        // per-file budget, so fixing this one alone will not move the score.
        style.paint(&format!("{:>PENALTY_WIDTH$}", "capped"), DIM)
    }
}

pub(crate) fn finding_line(finding: &Finding, layout: &Layout, style: &Style) -> String {
    let gap = " ".repeat(GAP);
    let mut out = " ".repeat(INDENT);
    out.push_str(&style.paint(
        &format!("{:>LINE_WIDTH$}", finding.highlight_range.start_line),
        DIM,
    ));
    out.push_str(&gap);
    out.push_str(&style.paint(&fit(&finding.category, layout.rule), CYAN));
    out.push_str(&gap);
    if layout.show_function {
        out.push_str(&style.paint(
            &fit(finding.function.as_deref().unwrap_or(""), layout.function),
            MAGENTA,
        ));
        out.push_str(&gap);
    }
    out.push_str(&fit(&finding.details, layout.details));
    out.push_str(&gap);
    out.push_str(&penalty_text(finding, style));
    out
}

/// All findings for a file, worst first.
pub(crate) fn file_findings(report: &FileReport) -> Vec<&Finding> {
    let mut findings = report.all_findings();
    findings.sort_by(|a, b| {
        b.penalty
            .partial_cmp(&a.penalty)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.highlight_range
                    .start_line
                    .cmp(&b.highlight_range.start_line)
            })
    });
    findings
}
