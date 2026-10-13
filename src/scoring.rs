//! Turning findings into a number.
//!
//! ```text
//! score = 10 - Σ penalties, floored at 1.0
//! ```
//!
//! Deliberately the whole model. Every point deducted traces to one finding with a
//! visible measurement, which is the property that makes the score usable as an
//! optimisation target.
//!
//! Two guards keep the arithmetic honest:
//!
//! *Per-rule file caps* stop one rule type from consuming the whole budget.
//!
//! *Largest-first cap allocation* means when a cap binds, the worst offenders keep
//! their full penalty and the marginal ones get trimmed, so fixing the worst function
//! always moves the score. Ties fall back to source order, which is why the detector
//! emission order in `detectors` is part of the contract.

use crate::detectors::{FileContext, RawFinding};
use crate::models::{round4, FileReport, Finding, FunctionReport, Range, SCORE_MAX, SCORE_MIN};
use crate::profile::Profile;
use std::cmp::Ordering;
use std::collections::HashMap;

/// Stable output order: down the file, then by rule name.
fn sort_key(finding: &Finding) -> (usize, usize, &str, &str) {
    (
        finding.highlight_range.start_line,
        finding.highlight_range.start_column,
        finding.category.as_str(),
        finding.function.as_deref().unwrap_or(""),
    )
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
}

/// Group findings by rule, preserving first-seen order so that cap allocation breaks
/// ties the same way the reference implementation's insertion-ordered dict did.
fn group_by_category(
    raw: Vec<RawFinding>,
) -> (Vec<&'static str>, HashMap<&'static str, Vec<RawFinding>>) {
    let mut order: Vec<&'static str> = Vec::new();
    let mut by_category: HashMap<&'static str, Vec<RawFinding>> = HashMap::new();
    for finding in raw {
        let category = finding.category;
        if !by_category.contains_key(category) {
            order.push(category);
        }
        by_category.entry(category).or_default().push(finding);
    }
    (order, by_category)
}

/// Attach a penalty to each finding, respecting per-rule file caps.
pub fn price_findings(raw: Vec<RawFinding>, profile: &Profile) -> Vec<Finding> {
    let (order, mut by_category) = group_by_category(raw);

    let mut priced: Vec<Finding> = Vec::new();
    for category in order {
        let mut group = by_category.remove(category).expect("category was inserted");
        let rule = *profile.rule(category);

        // Largest penalty first, so a binding cap trims the least severe. Stable sort
        // keeps discovery order for equal penalties.
        group.sort_by(|a, b| {
            let a_penalty = rule.penalty(a.value, a.threshold);
            let b_penalty = rule.penalty(b.value, b.threshold);
            match b_penalty.partial_cmp(&a_penalty) {
                Some(Ordering::Equal) | None => a.range.start_line.cmp(&b.range.start_line),
                Some(other) => other,
            }
        });

        let mut spent = 0.0;
        for finding in group {
            let wanted = rule.penalty(finding.value, finding.threshold);
            let allowed = wanted.min(rule.file_cap - spent).max(0.0);
            spent += allowed;
            priced.push(Finding {
                category: category.to_string(),
                details: finding.details,
                highlight_range: finding.range,
                penalty: round4(allowed),
                value: Some(finding.value),
                threshold: Some(finding.threshold),
                function: finding.function,
            });
        }
    }

    sort_findings(&mut priced);
    priced
}

/// Assemble a priced, grouped, scored report for one file.
pub fn score_file(ctx: &FileContext, raw: Vec<RawFinding>) -> FileReport {
    let findings = price_findings(raw, ctx.profile);

    let mut function_ranges: HashMap<&str, Range> = HashMap::new();
    for function in &ctx.functions {
        function_ranges.insert(function.name.as_str(), function.range);
    }

    let file_level: Vec<Finding> = findings
        .iter()
        .filter(|f| f.function.is_none())
        .cloned()
        .collect();

    // Insertion-ordered grouping by function name.
    let mut group_order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<Finding>> = HashMap::new();
    for finding in &findings {
        if let Some(name) = &finding.function {
            if !grouped.contains_key(name) {
                group_order.push(name.clone());
            }
            grouped
                .entry(name.clone())
                .or_default()
                .push(finding.clone());
        }
    }

    let mut function_reports: Vec<FunctionReport> = group_order
        .into_iter()
        .map(|name| {
            let mut smells = grouped.remove(&name).expect("name was inserted");
            sort_findings(&mut smells);
            let range = function_ranges
                .get(name.as_str())
                .copied()
                .unwrap_or(smells[0].highlight_range);
            FunctionReport {
                function: name,
                range,
                code_smells: smells,
            }
        })
        .collect();
    function_reports
        .sort_by(|a, b| (a.range.start_line, &a.function).cmp(&(b.range.start_line, &b.function)));

    let total: f64 = findings.iter().map(|f| f.penalty).sum();
    let score = (SCORE_MAX - total).max(SCORE_MIN);

    FileReport {
        path: ctx.path.clone(),
        language: ctx.spec.name.to_string(),
        loc: ctx.loc,
        score: round4(score),
        file_level_code_smells: file_level,
        function_level_code_smells: function_reports,
        error: None,
        syntax_errors: 0,
        suppressed: Vec::new(),
        directive_error: None,
        fingerprints: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finding at `line:column`, of rule `category`, filed under `function`.
    fn finding(at: (usize, usize), category: &str, function: Option<&str>) -> Finding {
        Finding {
            category: category.to_string(),
            details: String::new(),
            highlight_range: Range {
                start_line: at.0,
                start_column: at.1,
                end_line: at.0,
                end_column: at.1,
            },
            penalty: 0.0,
            value: None,
            threshold: None,
            function: function.map(str::to_string),
        }
    }

    #[test]
    fn findings_run_down_the_file_then_by_rule_then_by_function() {
        let expected = vec![
            finding((1, 9), "B", None),
            finding((2, 1), "B", Some("z")),
            finding((2, 3), "A", Some("a")),
            finding((2, 3), "A", Some("b")),
            finding((2, 3), "B", None),
        ];
        let mut shuffled: Vec<Finding> = expected.iter().rev().cloned().collect();
        shuffled.swap(0, 2);
        sort_findings(&mut shuffled);
        assert_eq!(shuffled, expected);
    }
}
