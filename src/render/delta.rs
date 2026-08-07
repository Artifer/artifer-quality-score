//! The `aqs delta` report, and the changed-file rows the gate report reuses.

use super::*;

/// Change labels, kept short so the column survives an 80-column terminal.
pub(crate) const LABEL_INTRODUCED: &str = "new";

pub(crate) const LABEL_DEGRADED: &str = "worse";

pub(crate) const LABEL_IMPROVED: &str = "better";

pub(crate) const LABEL_FIXED: &str = "fixed";

/// The introduced/degraded/improved/fixed rows under one changed file.
///
/// Takes the `FileDelta` rather than its four finding lists plus flags: the previous
/// signature needed seven arguments, which is exactly the smell this tool reports.
pub(crate) fn delta_rows(file: &FileDelta, layout: &Layout, style: &Style, include_positive: bool) {
    let gap = " ".repeat(GAP);
    let mut groups: Vec<(&str, &[Finding], &str)> = vec![
        (LABEL_INTRODUCED, &file.introduced, RED),
        (LABEL_DEGRADED, &file.degraded, YELLOW),
    ];
    if include_positive {
        groups.push((LABEL_IMPROVED, &file.improved, GREEN));
        groups.push((LABEL_FIXED, &file.fixed, GREEN));
    }

    for (label, findings, colour) in groups {
        for finding in findings {
            let mut row = " ".repeat(INDENT);
            // The change type is the whole point of a delta; it takes the fixed slot
            // the line number holds in a scan.
            row.push_str(&style.paint(&format!("{label:<PENALTY_WIDTH$}"), colour));
            row.push_str(&gap);
            row.push_str(&style.paint(&fit(&finding.category, layout.rule), CYAN));
            row.push_str(&gap);
            if layout.show_function {
                row.push_str(&style.paint(
                    &fit(finding.function.as_deref().unwrap_or(""), layout.function),
                    MAGENTA,
                ));
                row.push_str(&gap);
            }
            row.push_str(&fit(&finding.details, layout.details));
            println!("{row}");
        }
    }
}

/// The `  -1.20  path  (7.16 → 6.98)` heading for one changed file.
pub(crate) fn delta_heading(file: &FileDelta, style: &Style) {
    let colour = if file.is_regression() { RED } else { GREEN };
    let old = file.old_score.map(|s| format!("{s:.2}")).unwrap_or_else(|| "new".into());
    let new = file.new_score.map(|s| format!("{s:.2}")).unwrap_or_else(|| "deleted".into());
    println!(
        "  {}  {}{}",
        style.change(file.score_change),
        style.paint(&file.path, colour),
        style.paint(&format!("  ({old} → {new})"), DIM)
    );
}

pub fn render_delta(report: &DeltaReport, style: &Style, threshold: f64) {
    print_title(
        "Artifer Quality Score · delta",
        &format!("   baseline {} · v{}", report.baseline, report.version),
        style,
    );

    if report.files.is_empty() {
        println!("{}", style.paint("  No changed files to score.", DIM));
        println!();
        return;
    }

    println!(
        "  {}{}",
        style.change(report.score_change),
        style.paint(
            &format!(
                "   {:.2} → {:.2}   across {} changed file(s)",
                report.old_score,
                report.new_score,
                report.files.len()
            ),
            DIM
        )
    );
    println!();

    let layout = Layout::for_width(style.width);
    for file in &report.files {
        delta_heading(file, style);
        delta_rows(file, &layout, style, true);
        println!();
    }

    if report.score_change < -threshold.abs() {
        println!(
            "{}",
            style.paint(
                &format!(
                    "  Regression of {:.2} exceeds the allowed {:.2}",
                    report.score_change.abs(),
                    threshold.abs()
                ),
                &format!("{BOLD}{RED}")
            )
        );
        println!();
    }
}

