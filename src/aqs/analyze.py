"""File discovery and the analysis pipeline.

    bytes -> tree-sitter tree -> FileContext -> findings -> priced findings -> score

The pipeline is a pure function of (file bytes, profile, tool version). No git
history, no network, no clock, no filesystem-order dependence — which is what lets
`aqs` be used as a regression gate at all, and what makes the report cache in
`cache.py` sound.
"""

from __future__ import annotations

import os
from concurrent.futures import ProcessPoolExecutor
from concurrent.futures.process import BrokenProcessPool
from pathlib import Path

from tree_sitter import Node

from . import detectors, scoring
from .cache import NullCache, content_key
from .languages import LanguageSpec, spec_for_extension, spec_for_name
from .models import FileReport, ScanReport
from .profile import DEFAULT_PROFILE, Profile
from .scoring import aggregate_score
from .version import __version__

#: Below this many files, spawning workers costs more than it saves — each new
#: process re-imports the grammars from scratch. Measured on the corpus in the
#: README's benchmark table.
PARALLEL_FILE_THRESHOLD = 240

#: One analysis unit: (display path, language name, source bytes, profile).
AnalysisJob = tuple[str, str, bytes, Profile]


def parallel_map(worker, work: list, jobs: int | None) -> list:
    """Map `worker` over `work`, in parallel when that is worthwhile and possible.

    Falls back to serial on any pool failure. Parallelism is an optimisation, never
    a correctness requirement — and it can fail for reasons outside our control: a
    caller importing this module from a script with no ``if __name__ == "__main__"``
    guard cannot spawn children at all, and a sandbox may forbid it. Crashing there
    would be indefensible when the serial path gives an identical answer.

    Results are always in input order, so the output cannot reveal which path ran.
    """
    workers = jobs if jobs is not None else (os.cpu_count() or 1)
    if workers > 1 and len(work) >= PARALLEL_FILE_THRESHOLD:
        try:
            with ProcessPoolExecutor(max_workers=workers) as pool:
                chunk = max(1, len(work) // (workers * 4))
                return list(pool.map(worker, work, chunksize=chunk))
        except (RuntimeError, OSError, BrokenProcessPool):
            pass  # fall through to serial
    return [worker(job) for job in work]


def count_syntax_errors(root: Node) -> int:
    if not root.has_error:
        return 0
    errors = 0
    stack = [root]
    while stack:
        node = stack.pop()
        if node.type == "ERROR" or node.is_missing:
            errors += 1
            continue
        if node.has_error:
            stack.extend(node.children)
    return errors


def analyze_source(
    path: str, source: bytes, spec: LanguageSpec, profile: Profile
) -> FileReport:
    """Score one already-loaded source file."""
    tree = spec.parser().parse(source)
    ctx = detectors.build_context(path, spec, profile, tree.root_node, source)
    raw = detectors.detect(ctx)
    report = scoring.score_file(ctx, raw)
    return report.model_copy(
        update={"syntax_errors": count_syntax_errors(tree.root_node)}
    )


def analyze_job(job: AnalysisJob) -> FileReport:
    """Worker entry point. Module-level so it survives pickling."""
    path, language, source, profile = job
    spec = spec_for_name(language)
    assert spec is not None  # guaranteed by the caller
    return analyze_source(path, source, spec, profile)


def analyze_file(
    path: Path, profile: Profile, display_path: str | None = None
) -> FileReport:
    shown = display_path or path.as_posix()
    spec = spec_for_extension(path.suffix)
    if spec is None:
        return FileReport(
            path=shown,
            language="unknown",
            loc=0,
            score=10.0,
            error=f"unsupported file type: {path.suffix or '(none)'}",
        )
    try:
        source = path.read_bytes()
    except OSError as exc:
        return FileReport(
            path=shown, language=spec.name, loc=0, score=10.0, error=str(exc)
        )
    return analyze_source(shown, source, spec, profile)


def analyze_jobs(
    jobs_list: list[AnalysisJob],
    cache,
    workers: int | None,
) -> list[FileReport]:
    """Score a batch of jobs, serving what it can from `cache`.

    Cache lookups and writes happen here, in the parent, so workers stay pure.
    Output order matches input order.
    """
    slots: list[FileReport | None] = [None] * len(jobs_list)
    pending: list[AnalysisJob] = []
    pending_slots: list[int] = []
    pending_keys: list[str] = []

    for index, job in enumerate(jobs_list):
        path, _language, source, _profile = job
        key = content_key(path, source, cache.fingerprint) if cache.enabled else ""
        hit = cache.get(key) if cache.enabled else None
        if hit is not None:
            slots[index] = hit
            continue
        pending.append(job)
        pending_slots.append(index)
        pending_keys.append(key)

    results = parallel_map(analyze_job, pending, workers)
    for index, key, report in zip(pending_slots, pending_keys, results):
        slots[index] = report
        if cache.enabled:
            cache.put(key, report)
    cache.commit()

    return [report for report in slots if report is not None]


def discover(roots: list[Path], profile: Profile) -> list[Path]:
    """Every supported, non-excluded file under `roots`, in sorted order."""
    found: set[Path] = set()
    for root in roots:
        if root.is_file():
            if spec_for_extension(root.suffix) and not profile.is_excluded(
                root.as_posix()
            ):
                found.add(root)
            continue
        for candidate in root.rglob("*"):
            if not candidate.is_file():
                continue
            if spec_for_extension(candidate.suffix) is None:
                continue
            if profile.is_excluded(candidate.as_posix()):
                continue
            found.add(candidate)
    return sorted(found)


def scan(
    roots: list[Path],
    profile: Profile | None = None,
    relative_to: Path | None = None,
    jobs: int | None = None,
    cache=None,
) -> ScanReport:
    """Score every supported file under `roots`."""
    profile = profile or DEFAULT_PROFILE
    cache = cache if cache is not None else NullCache()
    paths = discover(roots, profile)

    unreadable: list[FileReport] = []
    jobs_list: list[AnalysisJob] = []
    for path in paths:
        display = _display(path, relative_to)
        spec = spec_for_extension(path.suffix)
        assert spec is not None  # discover() filtered on this
        try:
            source = path.read_bytes()
        except OSError as exc:
            unreadable.append(
                FileReport(
                    path=display, language=spec.name, loc=0, score=10.0, error=str(exc)
                )
            )
            continue
        jobs_list.append((display, spec.name, source, profile))

    reports = analyze_jobs(jobs_list, cache, jobs) + unreadable
    scored = [report for report in reports if report.error is None]
    return ScanReport(
        version=__version__,
        profile=profile.name,
        score=aggregate_score(scored),
        loc=sum(report.loc for report in scored),
        files=tuple(reports),
    )


def _display(path: Path, relative_to: Path | None) -> str:
    if relative_to is None:
        return path.as_posix()
    try:
        return path.resolve().relative_to(relative_to.resolve()).as_posix()
    except ValueError:
        return path.as_posix()
