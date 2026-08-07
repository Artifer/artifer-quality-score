//! Terminal rendering.
//!
//! Worst-first everywhere. A report that opens with the files you should look at is
//! useful in a TDD loop; one that opens with an alphabetical list is a wall to scroll
//! past.
//!
//! Column widths are computed by hand rather than by a table library: the line number
//! and the penalty must never be the columns that get dropped when the terminal is
//! narrow, and generic table solvers are free to collapse exactly those.

use crate::explain::{RuleDoc, RULE_DOCS};
use crate::models::{
    DeltaReport, FileDelta, FileReport, Finding, GateReport, ScanReport,
};
use crate::profile::Profile;

const HEALTHY: f64 = 8.0;
const WARNING: f64 = 4.0;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";

/// Change labels, kept short so the column survives an 80-column terminal.
const LABEL_INTRODUCED: &str = "new";
const LABEL_DEGRADED: &str = "worse";
const LABEL_IMPROVED: &str = "better";
const LABEL_FIXED: &str = "fixed";

/// The colour a score is shown in. A free function: it depends only on the number.
fn score_colour(score: f64) -> &'static str {
    if score >= HEALTHY {
        GREEN
    } else if score >= WARNING {
        YELLOW
    } else {
        RED
    }
}

pub struct Style {
    colour: bool,
    pub width: usize,
}

impl Style {
    pub fn new(colour: bool, width: usize) -> Self {
        Self { colour, width }
    }

    fn paint(&self, text: &str, code: &str) -> String {
        if self.colour {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    fn score(&self, score: f64, width: usize) -> String {
        let text = format!("{score:>width$.2}");
        if self.colour {
            format!("{BOLD}{}{}{RESET}", score_colour(score), text)
        } else {
            text
        }
    }

    fn change(&self, change: f64) -> String {
        if change > 0.0 {
            self.paint(&format!("+{change:.2}"), &format!("{BOLD}{GREEN}"))
        } else if change < 0.0 {
            self.paint(&format!("{change:.2}"), &format!("{BOLD}{RED}"))
        } else {
            self.paint("  0.00", DIM)
        }
    }
}

/// Pad or ellipsise `text` to exactly `width` display columns.
fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        let mut out: String = text.to_string();
        out.extend(std::iter::repeat(' ').take(width - chars.len()));
        return out;
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out: String = chars[..width - 1].iter().collect();
    out.push('…');
    out
}

struct Layout {
    rule: usize,
    function: usize,
    details: usize,
    show_function: bool,
}

const LINE_WIDTH: usize = 4;
const PENALTY_WIDTH: usize = 6;
const INDENT: usize = 3;
const GAP: usize = 2;

impl Layout {
    fn for_width(width: usize) -> Self {
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

fn penalty_text(finding: &Finding, style: &Style) -> String {
    if finding.penalty > 0.0 {
        style.paint(&format!("{:>PENALTY_WIDTH$}", format!("-{:.2}", finding.penalty)), RED)
    } else {
        // Capped out: the finding is real, but this rule has already spent its
        // per-file budget, so fixing this one alone will not move the score.
        style.paint(&format!("{:>PENALTY_WIDTH$}", "capped"), DIM)
    }
}

fn finding_line(finding: &Finding, layout: &Layout, style: &Style) -> String {
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
fn file_findings(report: &FileReport) -> Vec<&Finding> {
    let mut findings = report.all_findings();
    findings.sort_by(|a, b| {
        b.penalty
            .partial_cmp(&a.penalty)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.highlight_range.start_line.cmp(&b.highlight_range.start_line))
    });
    findings
}

fn render_scan_summary(report: &ScanReport, style: &Style) {
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
}

/// Files with findings, worst first.
fn flagged_files(report: &ScanReport) -> Vec<&FileReport> {
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

fn render_scan_files(flagged: &[&FileReport], shown: usize, style: &Style) {
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

/// The introduced/degraded/improved/fixed rows under one changed file.
///
/// Takes the `FileDelta` rather than its four finding lists plus flags: the previous
/// signature needed seven arguments, which is exactly the smell this tool reports.
fn delta_rows(file: &FileDelta, layout: &Layout, style: &Style, include_positive: bool) {
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
fn delta_heading(file: &FileDelta, style: &Style) {
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

/// The bold title line shared by every report.
fn print_title(title: &str, detail: &str, style: &Style) {
    println!();
    println!("  {}{}", style.paint(title, BOLD), style.paint(detail, DIM));
    println!();
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

/// One side of the gate comparison: `baseline  9.46   develop @ dbbd8f3c  77 files`.
fn gate_side(label: &str, score: f64, detail: &str, style: &Style) {
    println!(
        "  {}{}{}",
        style.paint(label, DIM),
        style.score(score, 0),
        style.paint(detail, DIM)
    );
}

fn render_gate_header(report: &GateReport, style: &Style) {
    print_title(
        "Artifer Quality Score · merge gate",
        &format!("   v{}", report.version),
        style,
    );
    gate_side(
        "baseline  ",
        report.baseline_score,
        &format!(
            "   {} @ {}   {} files · {} LoC",
            report.against,
            &report.baseline_revision[..8.min(report.baseline_revision.len())],
            report.baseline_file_count,
            thousands(report.baseline_loc)
        ),
        style,
    );
    gate_side(
        "current   ",
        report.current_score,
        &format!(
            "   {} files · {} LoC",
            report.current_file_count,
            thousands(report.current_loc)
        ),
        style,
    );
    println!("  {}{}", style.paint("change    ", DIM), style.change(report.score_change));
    println!();
}

fn render_gate_files(report: &GateReport, style: &Style) {
    // Files that changed without moving their score are not interesting here; showing
    // them buries the files that actually caused the drop.
    let moved: Vec<&FileDelta> =
        report.files.iter().filter(|f| f.score_change != 0.0).collect();
    let unchanged = report.files.len() - moved.len();

    if !moved.is_empty() {
        let layout = Layout::for_width(style.width);
        println!("  {}", style.paint("Changed files that moved the score", BOLD));
        for file in moved {
            delta_heading(file, style);
            delta_rows(file, &layout, style, false);
        }
        println!();
    }

    if unchanged > 0 {
        println!(
            "{}",
            style.paint(&format!("  {unchanged} other changed file(s) held their score"), DIM)
        );
        println!();
    }
}

fn render_gate_verdict(report: &GateReport, style: &Style) {
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

pub fn render_gate(report: &GateReport, style: &Style) {
    render_gate_header(report, style);
    render_gate_files(report, style);
    render_gate_verdict(report, style);
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub fn render_explain(doc: &RuleDoc, profile: &Profile, style: &Style) {
    println!();
    println!("  {}", style.paint(doc.category, &format!("{BOLD}{CYAN}")));
    println!();
    for chunk in wrap(doc.summary, 76) {
        println!("  {chunk}");
    }
    println!();
    for (label, body) in [("Why it matters", doc.why), ("How to fix it", doc.fix)] {
        println!("  {}", style.paint(label, BOLD));
        for chunk in wrap(body, 76) {
            println!("    {chunk}");
        }
        println!();
    }

    render_rule_config(doc, profile, style);
}

/// The thresholds and weight that govern one rule.
fn render_rule_config(doc: &RuleDoc, profile: &Profile, style: &Style) {
    if doc.thresholds.is_empty() {
        return;
    }
    let rule = profile.rule(doc.category);
    println!("  {}", style.paint("Configuration", BOLD));
    for key in doc.thresholds {
        let value = profile.threshold(key);
        println!("{}", style.paint(&format!("    {:<52} {}", key, number(value)), DIM));
    }
    println!(
        "{}",
        style.paint(
            &format!(
                "    {:<52} {}  (base {}, cap {}, file cap {})",
                "weight", rule.weight, rule.base, rule.cap, rule.file_cap
            ),
            DIM
        )
    );
    println!();
}

pub fn render_rule_list(profile: &Profile, style: &Style) {
    println!();
    println!(
        "  {:<38}{:>7}{:>7}  {}",
        style.paint("Rule", BOLD),
        style.paint("Weight", BOLD),
        style.paint("Max", BOLD),
        style.paint("What it measures", BOLD)
    );
    let available = style.width.saturating_sub(58).max(20);
    for doc in RULE_DOCS {
        let rule = profile.rule(doc.category);
        println!(
            "  {}{:>7.2}{:>7.2}  {}",
            style.paint(&fit(doc.category, 38), CYAN),
            rule.weight,
            rule.cap,
            fit(doc.summary, available)
        );
    }
    println!();
    println!(
        "{}",
        style.paint("  Run `aqs explain <rule>` for why it matters and how to fix it.", DIM)
    );
    println!();
}

pub fn render_thresholds(profile: &Profile, style: &Style) {
    println!();
    println!("  {:<54}{}", style.paint("Threshold", BOLD), style.paint("Value", BOLD));
    for (key, value) in &profile.thresholds {
        println!("  {}{}", style.paint(&fit(key, 54), CYAN), number(*value));
    }
    println!();
}

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

fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
