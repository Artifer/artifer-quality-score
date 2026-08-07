//! The `aqs scan` report: worst files first.

use super::*;

pub(crate) fn render_scan_summary(report: &ScanReport, style: &Style) {
    let mut summary = format!("  {}", style.score(report.score, 0));
    summary.push_str(&style.paint(
        &format!(
            " / 10   {} files · {} LoC · {} findings",
            report.scored_files().count(),
            thousands(report.loc),
            report.findings_count()
        ),
        DIM,
    ));
    println!("{summary}");

    let broken = report.scored_files().filter(|f| f.syntax_errors > 0).count();
    if broken > 0 {
        println!(
            "{}",
            style.paint(
                &format!("  {broken} file(s) had syntax errors; their scores are approximate"),
                YELLOW
            )
        );
    }
    let skipped = report.files.iter().filter(|f| f.error.is_some()).count();
    if skipped > 0 {
        println!("{}", style.paint(&format!("  {skipped} file(s) skipped"), DIM));
    }

    // Suppression is a decision, so it is never silent.
    let suppressed: usize = report.files.iter().map(|f| f.suppressed.len()).sum();
    if suppressed > 0 {
        println!(
            "{}",
            style.paint(
                &format!("  {suppressed} finding(s) suppressed by @aqs directives"),
                DIM
            )
        );
    }
    for file in &report.files {
        if let Some(error) = &file.directive_error {
            println!("{}", style.paint(&format!("  directive error: {error}"), RED));
        }
    }
}

/// Files with findings, worst first.
pub(crate) fn flagged_files(report: &ScanReport) -> Vec<&FileReport> {
    let mut flagged: Vec<&FileReport> =
        report.scored_files().filter(|f| f.finding_count() > 0).collect();
    flagged.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.loc.cmp(&a.loc))
            .then_with(|| a.path.cmp(&b.path))
    });
    flagged
}

pub(crate) fn render_scan_files(flagged: &[&FileReport], shown: usize, style: &Style) {
    let layout = Layout::for_width(style.width);
    for file in flagged.iter().take(shown) {
        println!(
            "  {}  {}{}",
            style.score(file.score, 5),
            style.paint(&file.path, score_colour(file.score)),
            style.paint(&format!("  ({} LoC)", file.loc), DIM)
        );
        for finding in file_findings(file) {
            println!("{}", finding_line(finding, &layout, style));
        }
        println!();
    }
}

pub fn render_scan(report: &ScanReport, style: &Style, limit: usize, show_all: bool) {
    print_title(
        "Artifer Quality Score",
        &format!("   profile {} · v{}", report.profile, report.version),
        style,
    );
    render_scan_summary(report, style);

    let flagged = flagged_files(report);
    if flagged.is_empty() {
        println!();
        println!(
            "  {}{}",
            style.paint("No findings. ", GREEN),
            style.paint("Every scanned file scored 10.00.", DIM)
        );
        println!();
        return;
    }

    let shown = if show_all { flagged.len() } else { limit.min(flagged.len()) };
    println!();
    println!(
        "  {}",
        style.paint(&format!("Lowest scoring files ({} of {})", shown, flagged.len()), BOLD)
    );
    println!();

    render_scan_files(&flagged, shown, style);

    if !show_all && flagged.len() > limit {
        println!(
            "{}",
            style.paint(
                &format!(
                    "  {} more file(s) with findings — pass --all to see them",
                    flagged.len() - limit
                ),
                DIM
            )
        );
        println!();
    }
}

