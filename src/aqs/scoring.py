"""Turning findings into a number.

    score = 10 - Σ penalties, floored at 1.0

Deliberately the whole model. Every point deducted traces to one finding with a
visible measurement, which is the property that makes the score usable as an
optimisation target: "7.2" on its own is not actionable, "7.2, and here are the
2.8 points and where they went" is.

Two guards keep the arithmetic honest:

*Per-rule file caps* stop one rule type from consuming the whole budget — a file
with forty mildly complex functions should not look identical to one with four
hundred, but nor should it drown out everything else the file gets wrong.

*Largest-first cap allocation* means when a cap binds, the worst offenders keep
their full penalty and the marginal ones get trimmed. Fixing the worst function
therefore always moves the score, which is not true if you trim from the top.
"""

from __future__ import annotations

from collections import defaultdict

from .detectors import FileContext, RawFinding
from .models import SCORE_MAX, SCORE_MIN, FileReport, Finding, FunctionReport
from .profile import Profile


def _sort_key(finding: Finding) -> tuple:
    """Stable output order: down the file, then by rule name."""
    return (
        finding.highlight_range.start_line,
        finding.highlight_range.start_column,
        finding.category,
        finding.function or "",
    )


def price_findings(raw: list[RawFinding], profile: Profile) -> list[Finding]:
    """Attach a penalty to each finding, respecting per-rule file caps."""
    by_category: dict[str, list[RawFinding]] = defaultdict(list)
    for finding in raw:
        by_category[finding.category].append(finding)

    priced: list[Finding] = []
    for category, group in by_category.items():
        rule = profile.rule(category)
        # Largest penalty first, so a binding cap trims the least severe.
        ordered = sorted(
            group,
            key=lambda f: (-rule.penalty(f.value, f.threshold), f.range.start_line),
        )
        spent = 0.0
        for finding in ordered:
            wanted = rule.penalty(finding.value, finding.threshold)
            allowed = max(0.0, min(wanted, rule.file_cap - spent))
            spent += allowed
            priced.append(
                Finding(
                    category=category,
                    details=finding.details,
                    highlight_range=finding.range,
                    penalty=round(allowed, 4),
                    value=finding.value,
                    threshold=finding.threshold,
                    function=finding.function,
                )
            )
    return sorted(priced, key=_sort_key)


def score_file(ctx: FileContext, raw: list[RawFinding]) -> FileReport:
    """Assemble a priced, grouped, scored report for one file."""
    findings = price_findings(raw, ctx.profile)

    function_ranges = {fn.name: fn.range for fn in ctx.functions}
    file_level = [f for f in findings if f.function is None]
    grouped: dict[str, list[Finding]] = defaultdict(list)
    for finding in findings:
        if finding.function is not None:
            grouped[finding.function].append(finding)

    function_reports = [
        FunctionReport(
            function=name,
            range=function_ranges.get(name, smells[0].highlight_range),
            code_smells=tuple(sorted(smells, key=_sort_key)),
        )
        for name, smells in grouped.items()
    ]
    function_reports.sort(key=lambda r: (r.range.start_line, r.function))

    total = sum(f.penalty for f in findings)
    score = max(SCORE_MIN, SCORE_MAX - total)

    return FileReport(
        path=ctx.path,
        language=ctx.spec.name,
        loc=ctx.loc,
        score=round(score, 4),
        file_level_code_smells=tuple(file_level),
        function_level_code_smells=tuple(function_reports),
    )


def aggregate_score(files: list[FileReport]) -> float:
    """LoC-weighted mean, so a 3000-line mess is not offset by ten clean stubs.

    This is the one piece of CodeScene's method that is fully published, and it
    is published because the naive alternative is actively misleading.
    """
    scored = [f for f in files if f.error is None and f.loc > 0]
    if not scored:
        return SCORE_MAX
    weighted = sum(f.score * f.loc for f in scored)
    total_loc = sum(f.loc for f in scored)
    return round(weighted / total_loc, 4)
