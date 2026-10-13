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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Range;

    fn finding(line: usize, penalty: f64) -> Finding {
        Finding {
            category: "Complex Method".into(),
            details: "cc = 14".into(),
            highlight_range: Range::line(line),
            penalty,
            value: None,
            threshold: None,
            function: Some("f".into()),
        }
    }

    #[test]
    fn a_narrow_terminal_drops_the_function_column_first() {
        let narrow = Layout::for_width(60);
        assert!(!narrow.show_function);
        assert_eq!(narrow.function, 0);
        assert!(Layout::for_width(120).show_function);

        // The line number and the penalty survive at any width.
        let line = finding_line(&finding(7, 0.5), &narrow, &Style::new(false, 60));
        assert!(line.starts_with("      7  "), "{line:?}");
        assert!(line.ends_with(" -0.50"), "{line:?}");
    }

    #[test]
    fn the_columns_share_what_the_fixed_ones_leave() {
        let columns = |width: usize| {
            let layout = Layout::for_width(width);
            (
                layout.rule,
                layout.function,
                layout.details,
                layout.show_function,
            )
        };
        // 21 columns are spoken for, which leaves 79 of a 100-wide terminal.
        assert_eq!(columns(100), (28, 20, 29, true));
        // The function column goes once fewer than 46 are left to share.
        assert_eq!(columns(67), (16, 11, 17, true));
        assert_eq!(columns(66), (20, 0, 25, false));
    }

    #[test]
    fn a_finding_past_the_rule_cap_says_so() {
        let style = Style::new(false, 100);
        assert_eq!(penalty_text(&finding(1, 0.0), &style), "capped");
    }

    #[test]
    fn equal_penalties_are_listed_in_line_order() {
        let mut report = FileReport::unsupported("f.py".into(), "python".into(), "x".into());
        report.file_level_code_smells = vec![finding(9, 0.5), finding(3, 0.5), finding(5, 0.8)];
        let lines: Vec<usize> = file_findings(&report)
            .iter()
            .map(|f| f.highlight_range.start_line)
            .collect();
        assert_eq!(lines, vec![5, 3, 9]);
    }
}
