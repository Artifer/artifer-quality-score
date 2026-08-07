"""Terminal rendering.

Worst-first everywhere. A report that opens with the files you should look at is
useful in a TDD loop; one that opens with an alphabetical list is a wall to
scroll past.
"""

from __future__ import annotations

from dataclasses import dataclass

from rich.console import Console
from rich.table import Table
from rich.text import Text

#: How a finding changed between baseline and working tree. Short labels so the
#: column survives an 80-column terminal, which is where this gets read.
CHANGE_LABELS = {
    "introduced": "new",
    "degraded": "worse",
    "improved": "better",
    "fixed": "fixed",
}

from .explain import RuleDoc
from .models import DeltaReport, FileReport, Finding, GateReport, ScanReport
from .profile import Profile

HEALTHY = 8.0
WARNING = 4.0


def score_style(score: float) -> str:
    if score >= HEALTHY:
        return "green"
    if score >= WARNING:
        return "yellow"
    return "red"


def score_text(score: float, width: int = 5) -> Text:
    return Text(f"{score:>{width}.2f}", style=f"bold {score_style(score)}")


def _change_text(change: float) -> Text:
    if change > 0:
        return Text(f"+{change:.2f}", style="bold green")
    if change < 0:
        return Text(f"{change:.2f}", style="bold red")
    return Text("  0.00", style="dim")


def _penalty_text(finding: Finding) -> Text:
    if finding.penalty > 0:
        return Text(f"-{finding.penalty:.2f}", style="red")
    # Capped out: the finding is real, but this rule has already spent its
    # per-file budget, so fixing this one alone will not move the score.
    return Text("capped", style="dim")


def _fit(text: str, width: int) -> str:
    """Pad or ellipsise `text` to exactly `width` columns."""
    if width <= 0:
        return ""
    if len(text) <= width:
        return text.ljust(width)
    if width == 1:
        return "…"
    return text[: width - 1] + "…"


@dataclass(frozen=True)
class _Layout:
    """Column widths for the findings list.

    Computed by hand rather than handed to rich's table solver: that solver is
    free to collapse a fixed-width column to nothing under pressure, and the line
    number and penalty are the two things that must never disappear.
    """

    rule: int
    function: int
    details: int
    show_function: bool

    LINE = 4
    PENALTY = 6
    INDENT = 3
    GAP = 2

    @classmethod
    def for_width(cls, width: int) -> "_Layout":
        fixed = cls.INDENT + cls.LINE + cls.PENALTY + cls.GAP * 4
        flexible = max(24, width - fixed)
        show_function = flexible >= 46
        if not show_function:
            rule = max(14, int(flexible * 0.45))
            return cls(rule=rule, function=0, details=flexible - rule, show_function=False)
        rule = min(34, max(14, int(flexible * 0.36)))
        function = min(24, max(8, int(flexible * 0.26)))
        return cls(
            rule=rule,
            function=function,
            details=max(10, flexible - rule - function - cls.GAP),
            show_function=True,
        )


def _finding_line(finding: Finding, layout: _Layout) -> Text:
    gap = " " * _Layout.GAP
    line = Text(" " * _Layout.INDENT)
    line.append(f"{finding.highlight_range.start_line:>{_Layout.LINE}}", style="dim")
    line.append(gap)
    line.append(_fit(finding.category, layout.rule), style="cyan")
    line.append(gap)
    if layout.show_function:
        line.append(_fit(finding.function or "", layout.function), style="magenta")
        line.append(gap)
    line.append(_fit(finding.details, layout.details))
    line.append(gap)
    penalty = _penalty_text(finding)
    line.append(penalty.plain.rjust(_Layout.PENALTY), style=penalty.style)
    return line


def _file_findings(report: FileReport) -> list[Finding]:
    """All findings for a file, worst first."""
    return sorted(
        report.all_findings,
        key=lambda f: (-f.penalty, f.highlight_range.start_line),
    )


def render_scan(
    report: ScanReport,
    console: Console,
    limit: int = 10,
    show_clean: bool = False,
) -> None:
    console.print()
    header = Text("  Artifer Quality Score", style="bold")
    header.append(f"   profile {report.profile} · v{report.version}", style="dim")
    console.print(header)
    console.print()

    summary = Text("  ")
    summary.append(score_text(report.score, width=0))
    summary.append(" / 10", style="dim")
    summary.append(
        f"   {len(report.scored_files)} files · {report.loc:,} LoC"
        f" · {report.findings_count} findings",
        style="dim",
    )
    console.print(summary)

    unsupported = [f for f in report.files if f.error is not None]
    broken = [f for f in report.scored_files if f.syntax_errors > 0]
    if broken:
        console.print(
            Text(
                f"  {len(broken)} file(s) had syntax errors; their scores are "
                "approximate",
                style="yellow",
            )
        )
    if unsupported:
        console.print(
            Text(f"  {len(unsupported)} file(s) skipped", style="dim")
        )

    flagged = [f for f in report.scored_files if f.all_findings]
    flagged.sort(key=lambda f: (f.score, -f.loc, f.path))

    if not flagged:
        console.print()
        console.print(Text("  No findings. ", style="green"), end="")
        console.print(Text("Every scanned file scored 10.00.", style="dim"))
        console.print()
        return

    shown = flagged if show_clean else flagged[:limit]
    console.print()
    console.print(Text(f"  Lowest scoring files ({len(shown)} of {len(flagged)})", style="bold"))
    console.print()

    for file_report in shown:
        line = Text("  ")
        line.append(score_text(file_report.score))
        line.append("  ")
        line.append(file_report.path, style=score_style(file_report.score))
        line.append(f"  ({file_report.loc} LoC)", style="dim")
        console.print(line)
        layout = _Layout.for_width(console.width)
        for finding in _file_findings(file_report):
            console.print(_finding_line(finding, layout))
        console.print()

    if not show_clean and len(flagged) > limit:
        console.print(
            Text(
                f"  {len(flagged) - limit} more file(s) with findings — "
                "pass --all to see them",
                style="dim",
            )
        )
        console.print()


def render_delta(report: DeltaReport, console: Console, threshold: float) -> None:
    console.print()
    header = Text("  Artifer Quality Score · delta", style="bold")
    header.append(f"   baseline {report.baseline} · v{report.version}", style="dim")
    console.print(header)
    console.print()

    if not report.files:
        console.print(Text("  No changed files to score.", style="dim"))
        console.print()
        return

    summary = Text("  ")
    summary.append(_change_text(report.score_change))
    summary.append(
        f"   {report.old_score:.2f} → {report.new_score:.2f}"
        f"   across {len(report.files)} changed file(s)",
        style="dim",
    )
    console.print(summary)
    console.print()

    for file_delta in report.files:
        line = Text("  ")
        line.append(_change_text(file_delta.score_change))
        line.append("  ")
        style = "red" if file_delta.is_regression else "green"
        line.append(file_delta.path, style=style)
        old = "new" if file_delta.old_score is None else f"{file_delta.old_score:.2f}"
        new = (
            "deleted"
            if file_delta.new_score is None
            else f"{file_delta.new_score:.2f}"
        )
        line.append(f"  ({old} → {new})", style="dim")
        console.print(line)

        layout = _Layout.for_width(console.width)
        gap = " " * _Layout.GAP
        for label, findings, style in (
            ("introduced", file_delta.introduced, "red"),
            ("degraded", file_delta.degraded, "yellow"),
            ("improved", file_delta.improved, "green"),
            ("fixed", file_delta.fixed, "green"),
        ):
            for finding in findings:
                row = Text(" " * _Layout.INDENT)
                # The change type is the whole point of a delta; it takes the
                # fixed slot the line number holds in a scan.
                row.append(f"{CHANGE_LABELS[label]:<{_Layout.PENALTY}}", style=style)
                row.append(gap)
                row.append(_fit(finding.category, layout.rule), style="cyan")
                row.append(gap)
                if layout.show_function:
                    row.append(
                        _fit(finding.function or "", layout.function), style="magenta"
                    )
                    row.append(gap)
                row.append(_fit(finding.details, layout.details))
                console.print(row)
        console.print()

    if report.score_change < -abs(threshold):
        console.print(
            Text(
                f"  Regression of {abs(report.score_change):.2f} exceeds the "
                f"allowed {abs(threshold):.2f}",
                style="bold red",
            )
        )
        console.print()


def render_gate(report: GateReport, console: Console) -> None:
    console.print()
    header = Text("  Artifer Quality Score · merge gate", style="bold")
    header.append(f"   v{report.version}", style="dim")
    console.print(header)
    console.print()

    baseline = Text("  baseline  ", style="dim")
    baseline.append(score_text(report.baseline_score, width=0))
    baseline.append(
        f"   {report.against} @ {report.baseline_revision[:8]}"
        f"   {report.baseline_file_count} files · {report.baseline_loc:,} LoC",
        style="dim",
    )
    console.print(baseline)

    current = Text("  current   ", style="dim")
    current.append(score_text(report.current_score, width=0))
    current.append(
        f"   {report.current_file_count} files · {report.current_loc:,} LoC",
        style="dim",
    )
    console.print(current)

    change = Text("  change    ", style="dim")
    change.append(_change_text(report.score_change))
    console.print(change)
    console.print()

    # Files that changed without moving their score are not interesting here;
    # showing them buries the two files that actually caused the drop.
    moved = [f for f in report.files if f.score_change != 0]
    unchanged = len(report.files) - len(moved)

    if moved:
        layout = _Layout.for_width(console.width)
        gap = " " * _Layout.GAP
        console.print(Text("  Changed files that moved the score", style="bold"))
        for file_delta in moved:
            line = Text("  ")
            line.append(_change_text(file_delta.score_change))
            line.append("  ")
            line.append(
                file_delta.path,
                style="red" if file_delta.is_regression else "green",
            )
            old = "new" if file_delta.old_score is None else f"{file_delta.old_score:.2f}"
            new = (
                "deleted"
                if file_delta.new_score is None
                else f"{file_delta.new_score:.2f}"
            )
            line.append(f"  ({old} → {new})", style="dim")
            console.print(line)
            for label, findings, style in (
                ("introduced", file_delta.introduced, "red"),
                ("degraded", file_delta.degraded, "yellow"),
            ):
                for finding in findings:
                    row = Text(" " * _Layout.INDENT)
                    row.append(
                        f"{CHANGE_LABELS[label]:<{_Layout.PENALTY}}", style=style
                    )
                    row.append(gap)
                    row.append(_fit(finding.category, layout.rule), style="cyan")
                    row.append(gap)
                    if layout.show_function:
                        row.append(
                            _fit(finding.function or "", layout.function),
                            style="magenta",
                        )
                        row.append(gap)
                    row.append(_fit(finding.details, layout.details))
                    console.print(row)
        console.print()

    if unchanged:
        console.print(
            Text(
                f"  {unchanged} other changed file(s) held their score", style="dim"
            )
        )
        console.print()

    if report.passed:
        message = Text("  PASS", style="bold green")
        if report.score_change > 0:
            message.append(
                f"  repository score improved by {report.score_change:.2f}", style="dim"
            )
        else:
            message.append("  repository score did not go down", style="dim")
    else:
        message = Text("  FAIL", style="bold red")
        message.append(
            f"  repository score dropped {abs(report.score_change):.2f}"
            f" (allowed {abs(report.tolerance):.2f})",
            style="dim",
        )
    console.print(message)
    console.print()


def render_explain(doc: RuleDoc, profile: Profile, console: Console) -> None:
    console.print()
    console.print(Text(f"  {doc.category}", style="bold cyan"))
    console.print()
    for chunk in _wrap(doc.summary, 76):
        console.print(Text(f"  {chunk}"))
    console.print()
    for label, body in (("Why it matters", doc.why), ("How to fix it", doc.fix)):
        console.print(Text(f"  {label}", style="bold"))
        for chunk in _wrap(body, 76):
            console.print(Text(f"    {chunk}"))
        console.print()

    if doc.thresholds:
        rule = profile.rules[doc.category]
        console.print(Text("  Configuration", style="bold"))
        for key in doc.thresholds:
            value = profile.thresholds[key]
            pretty = int(value) if float(value).is_integer() else value
            console.print(Text(f"    {key:<52} {pretty}", style="dim"))
        console.print(
            Text(
                f"    {'weight':<52} {rule.weight}  "
                f"(base {rule.base}, cap {rule.cap}, file cap {rule.file_cap})",
                style="dim",
            )
        )
        console.print()


def render_rule_list(profile: Profile, console: Console) -> None:
    from .explain import RULE_DOCS

    table = Table(
        show_header=True, header_style="bold", box=None, padding=(0, 2), pad_edge=False
    )
    table.add_column("Rule", style="cyan", no_wrap=True)
    table.add_column("Weight", justify="right")
    table.add_column("Max", justify="right")
    # One line per rule keeps this a scannable index; `aqs explain <rule>` is
    # where the full text lives.
    table.add_column("What it measures", no_wrap=True, overflow="ellipsis")

    for category, doc in RULE_DOCS.items():
        rule = profile.rules[category]
        weight_style = "dim" if rule.weight == 0 else ""
        table.add_row(
            category,
            Text(f"{rule.weight:.2f}", style=weight_style),
            Text(f"{rule.cap:.2f}", style="dim"),
            doc.summary,
        )
    console.print()
    console.print(table)
    console.print(
        Text(
            "  Run `aqs explain <rule>` for why it matters and how to fix it.",
            style="dim",
        )
    )
    console.print()


def render_thresholds(profile: Profile, console: Console) -> None:
    table = Table(
        show_header=True, header_style="bold", box=None, padding=(0, 2), pad_edge=False
    )
    table.add_column("Threshold", style="cyan", no_wrap=True)
    table.add_column("Value", justify="right")
    for key in sorted(profile.thresholds):
        value = profile.thresholds[key]
        pretty = int(value) if float(value).is_integer() else value
        table.add_row(key, str(pretty))
    console.print()
    console.print(table)
    console.print()


def render_check(report: ScanReport, console: Console) -> None:
    """Lint-style one-line-per-finding output, for editors and grep."""
    for file_report in report.files:
        for finding in sorted(
            file_report.all_findings,
            key=lambda f: (f.highlight_range.start_line, f.category),
        ):
            location = (
                f"{file_report.path}:{finding.highlight_range.start_line}"
                f":{finding.highlight_range.start_column}"
            )
            where = f" [{finding.function}]" if finding.function else ""
            console.print(
                f"{location}: {finding.category}: {finding.details}"
                f"{where} (-{finding.penalty:.2f})",
                highlight=False,
                soft_wrap=True,
            )


def _wrap(text: str, width: int) -> list[str]:
    import textwrap

    return textwrap.wrap(" ".join(text.split()), width=width) or [""]
