"""Changed-file comparison against a git baseline — the fast local loop.

`aqs delta` scores only what changed, so it runs in well under a second on any
repo size and is cheap enough for a pre-commit hook or a TDD watch loop.

Its aggregate covers *the changed files only*. That is the right question for
"did I make this worse", but it is **not** the repo score: adding a perfectly
reasonable large file can lower a repo's LoC-weighted average without any file
regressing. For the "repo score must never go down" gate, use `aqs gate`, which
scores both trees in full.

Findings are matched between baseline and working tree by (rule, function), never
by line number, so inserting a line above a function does not read as a fresh
smell.
"""

from __future__ import annotations

from pathlib import Path

from .analyze import analyze_source, parallel_map
from .gitio import (
    GitError,
    changed_files,
    read_blobs,
    repo_root,
    resolve_baseline,
)
from .languages import spec_for_extension, spec_for_name
from .models import SCORE_MAX, DeltaReport, FileDelta, FileReport, Finding
from .profile import Profile
from .scoring import aggregate_score
from .version import __version__

__all__ = ["GitError", "compute_delta", "repo_root"]


def _finding_key(finding: Finding) -> tuple[str, str]:
    return (finding.category, finding.function or "<file>")


def compare_reports(
    old: FileReport | None, new: FileReport | None
) -> tuple[tuple[Finding, ...], ...]:
    """Split findings into introduced / fixed / degraded / improved."""
    old_findings = {_finding_key(f): f for f in (old.all_findings if old else ())}
    new_findings = {_finding_key(f): f for f in (new.all_findings if new else ())}

    introduced = [f for key, f in new_findings.items() if key not in old_findings]
    fixed = [f for key, f in old_findings.items() if key not in new_findings]
    degraded, improved = [], []
    for key, current in new_findings.items():
        previous = old_findings.get(key)
        if previous is None:
            continue
        if current.penalty > previous.penalty:
            degraded.append(current)
        elif current.penalty < previous.penalty:
            improved.append(current)

    def order(items: list[Finding]) -> tuple[Finding, ...]:
        return tuple(
            sorted(items, key=lambda f: (f.highlight_range.start_line, f.category))
        )

    return order(introduced), order(fixed), order(degraded), order(improved)


def _analyze_pair(
    job: tuple[str, str, bytes | None, bytes | None, Profile],
) -> tuple[FileDelta, FileReport | None, FileReport | None]:
    """Worker entry point. Module-level so it survives pickling."""
    relative, language, old_source, new_source, profile = job
    spec = spec_for_name(language)
    assert spec is not None  # guaranteed by the caller

    old = (
        analyze_source(relative, old_source, spec, profile)
        if old_source is not None
        else None
    )
    new = (
        analyze_source(relative, new_source, spec, profile)
        if new_source is not None
        else None
    )

    introduced, fixed, degraded, improved = compare_reports(old, new)
    old_score = old.score if old else None
    new_score = new.score if new else None
    # An absent side counts as a clean 10.0, matching how CodeScene compares
    # against a file that did not exist.
    change = (new_score if new_score is not None else SCORE_MAX) - (
        old_score if old_score is not None else SCORE_MAX
    )

    delta = FileDelta(
        path=relative,
        old_score=old_score,
        new_score=new_score,
        score_change=round(change, 4),
        introduced=introduced,
        fixed=fixed,
        degraded=degraded,
        improved=improved,
    )
    return delta, old, new


def compute_delta(
    root: Path,
    profile: Profile,
    baseline: str = "HEAD",
    staged: bool = False,
    paths: list[str] | None = None,
    merge_base: bool = False,
    jobs: int | None = None,
) -> DeltaReport:
    """Score every changed file twice — at the baseline, and as it stands now."""
    repo = repo_root(root)
    resolved = resolve_baseline(repo, baseline, merge_base)
    targets = paths if paths is not None else changed_files(repo, resolved, staged)

    candidates = [
        (relative, spec.name)
        for relative in targets
        for spec in [spec_for_extension(Path(relative).suffix)]
        if spec is not None and not profile.is_excluded(relative)
    ]

    # One batched git call per side, rather than two subprocesses per file. On a
    # large branch diff that dominates everything else.
    old_blobs = read_blobs(repo, [f"{resolved}:{path}" for path, _ in candidates])
    staged_blobs = (
        read_blobs(repo, [f":{path}" for path, _ in candidates]) if staged else {}
    )

    work: list[tuple[str, str, bytes | None, bytes | None, Profile]] = []
    for relative, language in candidates:
        old_source = old_blobs.get(f"{resolved}:{relative}")
        if staged:
            new_source = staged_blobs.get(f":{relative}")
        else:
            absolute = repo / relative
            new_source = absolute.read_bytes() if absolute.is_file() else None
        if old_source is None and new_source is None:
            continue
        work.append((relative, language, old_source, new_source, profile))

    results = parallel_map(_analyze_pair, work, jobs)
    deltas = [delta for delta, _, _ in results]
    old_reports = [old for _, old, _ in results if old is not None]
    new_reports = [new for _, _, new in results if new is not None]

    label = f"{baseline} (merge-base {resolved[:8]})" if merge_base else resolved
    return DeltaReport(
        version=__version__,
        profile=profile.name,
        baseline=f"{label} (staged)" if staged else label,
        old_score=aggregate_score(old_reports),
        new_score=aggregate_score(new_reports),
        score_change=round(
            aggregate_score(new_reports) - aggregate_score(old_reports), 4
        ),
        files=tuple(sorted(deltas, key=lambda d: (d.score_change, d.path))),
    )
