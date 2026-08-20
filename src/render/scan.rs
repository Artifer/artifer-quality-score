//! The `aqs scan` report: worst files first.

use super::*;

/// The score and what it was measured over.
fn headline(report: &ScanReport, style: &Style) {
    // The observation count sits on the headline rather than in a footnote: an
    // observation costs no points, so this line is the only thing standing between it
    // and being read as "nothing left to do".
    let observations = match report.open_observations() {
        0 => String::new(),
        n => format!(" · {n} observations"),
    };
    let counts = format!(
        " / 10   {} files · {} LoC · {} findings{}",
        report.scored_files().count(),
        thousands(report.loc),
        report.findings_count(),
        observations
    );
    println!(
        "  {}{}",
        style.score(report.score, 0),
        style.paint(&counts, DIM)
    );
}

/// Reasons to trust the number a little less.
fn caveats(report: &ScanReport, style: &Style) {
    let broken = report
        .scored_files()
        .filter(|f| f.syntax_errors > 0)
        .count();
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
        println!(
            "{}",
            style.paint(&format!("  {skipped} file(s) skipped"), DIM)
        );
    }
}

/// What this branch chose to ignore. Never silent — a suppression is a decision, and a
/// dismissed observation is the same decision about the unscored half.
fn decisions(report: &ScanReport, style: &Style) {
    let suppressed: usize = report.files.iter().map(|f| f.suppressed.len()).sum();
    let dismissed = report.observations.len() - report.open_observations();
    let counted = [
        (suppressed, "finding(s) suppressed"),
        (dismissed, "observation(s) dismissed"),
    ];
    let parts: Vec<String> = counted
        .iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect();
    if !parts.is_empty() {
        println!(
            "{}",
            style.paint(&format!("  {} by @aqs directives", parts.join(", ")), DIM)
        );
    }
}

pub(crate) fn render_scan_summary(report: &ScanReport, style: &Style) {
    headline(report, style);
    caveats(report, style);
    decisions(report, style);
    for file in &report.files {
        if let Some(error) = &file.directive_error {
            println!(
                "{}",
                style.paint(&format!("  directive error: {error}"), RED)
            );
        }
    }
}

/// Files with findings, worst first.
pub(crate) fn flagged_files(report: &ScanReport) -> Vec<&FileReport> {
    let mut flagged: Vec<&FileReport> = report
        .scored_files()
        .filter(|f| f.finding_count() > 0)
        .collect();
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

/// The unscored cross-file findings, listed with every site.
///
/// Deliberately printed after the file list and never gated on there being findings: the
/// case this exists for — one function copied into two files — leaves every file at 10.00
/// and the file list empty. A channel that only appears when the score is already bad
/// would miss the thing it was built for.
pub(crate) fn render_observations(report: &ScanReport, style: &Style) {
    let open: Vec<_> = report.observations.iter().filter(|o| o.is_open()).collect();
    if open.is_empty() {
        return;
    }
    println!(
        "  {}{}",
        style.paint(&format!("Observations ({})", open.len()), BOLD),
        style.paint(
            "  ·  not scored — fix, or dismiss with @aqs and a reason",
            DIM
        )
    );
    println!();

    for observation in open {
        println!(
            "  {}  {}",
            style.paint(&observation.category, MAGENTA),
            style.paint(&observation.details, DIM)
        );
        for site in &observation.sites {
            let name = site.function.as_deref().unwrap_or("");
            println!(
                "      {}{}",
                style.paint(&format!("{}:{}", site.path, site.line), CYAN),
                style.paint(&format!("  {name}"), DIM)
            );
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
    println!();
    if flagged.is_empty() {
        println!(
            "  {}{}",
            style.paint("No findings. ", GREEN),
            style.paint("Every scanned file scored 10.00.", DIM)
        );
        println!();
    } else {
        let shown = if show_all {
            flagged.len()
        } else {
            limit.min(flagged.len())
        };
        println!(
            "  {}",
            style.paint(
                &format!("Lowest scoring files ({} of {})", shown, flagged.len()),
                BOLD
            )
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

    render_observations(report, style);
}
