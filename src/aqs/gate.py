"""The merge gate: the repository score must never go down.

This scores **both trees in full**, which is the only way to enforce that rule
honestly. A changed-files comparison cannot see a regression caused by weight
shift: add an 800-line file scoring 8.5 to a repo averaging 9.3 and nothing has
regressed, every file is fine, and the repo score still falls. `aqs delta` would
report zero; this reports the drop.

The baseline is the **merge base** with the target branch by default, so the gate
judges only your own changes. Comparing against the tip of `develop` would fail
your branch for regressions other people merged while you were working.
"""

from __future__ import annotations

from pathlib import Path

from .cache import open_cache
from .delta import compare_reports
from .gitio import changed_files, repo_root, resolve_baseline, scan_revision
from .models import FileDelta, GateReport, ScanReport
from .profile import Profile
from .version import __version__


def _file_deltas(
    baseline: ScanReport, current: ScanReport, changed: set[str]
) -> tuple[FileDelta, ...]:
    """Per-file detail for changed files, so a failing gate is actionable."""
    old_by_path = {report.path: report for report in baseline.files}
    new_by_path = {report.path: report for report in current.files}

    deltas: list[FileDelta] = []
    for path in sorted(changed):
        old = old_by_path.get(path)
        new = new_by_path.get(path)
        if old is None and new is None:
            continue
        introduced, fixed, degraded, improved = compare_reports(old, new)
        old_score = old.score if old else None
        new_score = new.score if new else None
        if old_score is None and new_score is None:
            continue
        change = (new_score if new_score is not None else 10.0) - (
            old_score if old_score is not None else 10.0
        )
        deltas.append(
            FileDelta(
                path=path,
                old_score=old_score,
                new_score=new_score,
                score_change=round(change, 4),
                introduced=introduced,
                fixed=fixed,
                degraded=degraded,
                improved=improved,
            )
        )
    return tuple(sorted(deltas, key=lambda d: (d.score_change, d.path)))


def compute_gate(
    root: Path,
    profile: Profile,
    against: str = "origin/develop",
    tolerance: float = 0.0,
    merge_base: bool = True,
    jobs: int | None = None,
    current: ScanReport | None = None,
    use_cache: bool = True,
) -> GateReport:
    """Compare the whole repository against a baseline revision.

    `current` defaults to scanning the working tree via HEAD. Pass an explicit
    ScanReport to gate uncommitted work instead.
    """
    repo = repo_root(root)
    baseline_revision = resolve_baseline(repo, against, merge_base)

    with open_cache(repo, profile, enabled=use_cache) as cache:
        baseline = scan_revision(
            repo, baseline_revision, profile, jobs=jobs, cache=cache
        )
        if current is None:
            current = scan_revision(repo, "HEAD", profile, jobs=jobs, cache=cache)

    changed = set(changed_files(repo, baseline_revision, staged=False))
    score_change = round(current.score - baseline.score, 4)

    return GateReport(
        version=__version__,
        profile=profile.name,
        against=against,
        baseline_revision=baseline_revision,
        baseline_score=baseline.score,
        baseline_loc=baseline.loc,
        baseline_file_count=len(baseline.scored_files),
        current_score=current.score,
        current_loc=current.loc,
        current_file_count=len(current.scored_files),
        score_change=score_change,
        tolerance=tolerance,
        files=_file_deltas(baseline, current, changed),
    )
