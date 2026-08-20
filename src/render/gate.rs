//! The `aqs gate` report: two whole-tree scores and a verdict.

use super::*;

pub(crate) fn render_gate_header(report: &GateReport, style: &Style) {
    print_title(
        "Artifer Quality Score · merge gate",
        &format!("   v{}", report.version),
        style,
    );

    // The two sides are the same shape, so they are data rather than two call sites
    // with three loose arguments each.
    let revision = &report.baseline_revision[..8.min(report.baseline_revision.len())];
    let sides = [
        (
            "baseline  ",
            report.baseline_score,
            format!(
                "   {} @ {}   {} files · {} LoC",
                report.against,
                revision,
                report.baseline_file_count,
                thousands(report.baseline_loc)
            ),
        ),
        (
            "current   ",
            report.current_score,
            format!(
                "   {} files · {} LoC",
                report.current_file_count,
                thousands(report.current_loc)
            ),
        ),
    ];
    for (label, score, detail) in sides {
        println!(
            "  {}{}{}",
            style.paint(label, DIM),
            style.score(score, 0),
            style.paint(&detail, DIM)
        );
    }
    println!(
        "  {}{}",
        style.paint("change    ", DIM),
        style.change(report.score_change)
    );
    println!();
}

pub(crate) fn render_gate_files(report: &GateReport, style: &Style) {
    // Files that changed without moving their score are not interesting here; showing
    // them buries the files that actually caused the drop.
    let moved: Vec<&FileDelta> = report
        .files
        .iter()
        .filter(|f| f.score_change != 0.0)
        .collect();
    let unchanged = report.files.len() - moved.len();

    if !moved.is_empty() {
        let layout = Layout::for_width(style.width);
        println!(
            "  {}",
            style.paint("Changed files that moved the score", BOLD)
        );
        for file in moved {
            delta_heading(file, style);
            delta_rows(file, &layout, style, false);
        }
        println!();
    }

    if unchanged > 0 {
        println!(
            "{}",
            style.paint(
                &format!("  {unchanged} other changed file(s) held their score"),
                DIM
            )
        );
        println!();
    }
}

pub(crate) fn render_gate_verdict(report: &GateReport, style: &Style) {
    if report.passed() {
        let detail = if report.score_change > 0.0 {
            format!("  repository score improved by {:.2}", report.score_change)
        } else {
            "  repository score did not go down".to_string()
        };
        println!(
            "  {}{}",
            style.paint("PASS", &format!("{BOLD}{GREEN}")),
            style.paint(&detail, DIM)
        );
    } else {
        println!(
            "  {}{}",
            style.paint("FAIL", &format!("{BOLD}{RED}")),
            style.paint(
                &format!(
                    "  repository score dropped {:.2} (allowed {:.2})",
                    report.score_change.abs(),
                    report.tolerance.abs()
                ),
                DIM
            )
        );
    }
    println!();
}

/// Observations this branch added.
///
/// Printed on a pass as well as a failure, and deliberately so. The branch this exists to
/// catch — one that copies a function into a second file — moves no score at all, so the
/// only run on which it would ever be seen is a passing one.
pub(crate) fn render_gate_observations(report: &GateReport, style: &Style) {
    if report.observations_introduced.is_empty() {
        return;
    }
    println!(
        "  {}{}",
        style.paint(
            &format!(
                "Observations introduced ({})",
                report.observations_introduced.len()
            ),
            BOLD
        ),
        style.paint("  ·  not scored, and not gated on", DIM)
    );
    for observation in &report.observations_introduced {
        println!(
            "    {}  {}",
            style.paint(&observation.category, MAGENTA),
            style.paint(&observation.details, DIM)
        );
        for site in &observation.sites {
            println!(
                "{}",
                style.paint(
                    &format!(
                        "        {}:{}  {}",
                        site.path,
                        site.line,
                        site.function.as_deref().unwrap_or("")
                    ),
                    DIM
                )
            );
        }
    }
    println!();
}

pub fn render_gate(report: &GateReport, style: &Style) {
    render_gate_header(report, style);
    render_gate_files(report, style);
    render_gate_observations(report, style);
    render_gate_verdict(report, style);
}
