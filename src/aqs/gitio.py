"""Git primitives, and scoring a whole tree at an arbitrary revision.

Scoring a past revision never checks anything out. `git ls-tree` lists the paths
and `git cat-file --batch` streams the blobs through a single process, so a
baseline scan costs no temp directory, no worktree, and no second clone.

Blobs are read and analysed in chunks so peak memory stays bounded by the chunk
rather than by the size of the repository.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from typing import Iterator

from .analyze import AnalysisJob, analyze_jobs
from .cache import NullCache
from .languages import spec_for_extension
from .models import FileReport, ScanReport
from .profile import Profile
from .scoring import aggregate_score
from .version import __version__

#: Paths per batched `cat-file` round trip. Bounds peak memory on large repos while
#: staying large enough that most repositories need only one worker pool.
BLOB_CHUNK_SIZE = 2048


class GitError(RuntimeError):
    """Raised when git is unavailable or the repository/ref cannot be used."""


def git(repo: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", *args], cwd=repo, capture_output=True, text=True, check=False
        )
    except FileNotFoundError as exc:  # pragma: no cover - environment dependent
        raise GitError("git is not installed or not on PATH") from exc
    if result.returncode != 0:
        raise GitError(result.stderr.strip() or f"git {' '.join(args)} failed")
    return result.stdout


def repo_root(start: Path) -> Path:
    directory = start if start.is_dir() else start.parent
    output = git(directory, "rev-parse", "--show-toplevel").strip()
    if not output:
        raise GitError(f"{directory} is not inside a git repository")
    return Path(output)


def resolve_baseline(repo: Path, ref: str, use_merge_base: bool) -> str:
    """The commit to compare against.

    For a merge gate this must be the *merge base*, not the branch tip. Comparing
    against the tip of `develop` would charge your branch for every change other
    people merged while you were working; the common ancestor isolates your diff.
    """
    if not use_merge_base:
        # Always resolve to a concrete SHA: a report that records "HEAD~5" as its
        # baseline is not reproducible tomorrow.
        resolved = git(repo, "rev-parse", "--verify", f"{ref}^{{commit}}").strip()
        if not resolved:
            raise GitError(f"cannot resolve {ref}")
        return resolved
    merge_base = git(repo, "merge-base", "HEAD", ref).strip()
    if not merge_base:
        raise GitError(f"no common ancestor between HEAD and {ref}")
    return merge_base


def changed_files(repo: Path, baseline: str, staged: bool) -> list[str]:
    """Repo-relative paths that differ from the baseline.

    Deletions are included deliberately: removing a bad file is a real
    improvement, and a gate that cannot see it would under-report progress.
    Untracked files are included in working-tree mode, since freshly generated
    code is exactly what this exists to check and it has no baseline at all.
    """
    diff_filter = "--diff-filter=ACMRD"
    if staged:
        raw = git(repo, "diff", "--name-only", "--cached", diff_filter, baseline)
        return sorted({line for line in raw.splitlines() if line.strip()})

    tracked = git(repo, "diff", "--name-only", diff_filter, baseline)
    untracked = git(repo, "ls-files", "--others", "--exclude-standard")
    return sorted({line for line in (tracked + untracked).splitlines() if line.strip()})


def read_blobs(repo: Path, revisions: list[str]) -> dict[str, bytes | None]:
    """Read many git blobs through a single `git cat-file --batch` process.

    Returns None for revisions that do not exist, which is how a newly added file
    is represented.
    """
    if not revisions:
        return {}

    request = "".join(f"{revision}\n" for revision in revisions).encode()
    try:
        process = subprocess.run(
            ["git", "cat-file", "--batch"],
            cwd=repo,
            input=request,
            capture_output=True,
            check=False,
        )
    except FileNotFoundError as exc:  # pragma: no cover
        raise GitError("git is not installed or not on PATH") from exc

    blobs: dict[str, bytes | None] = {}
    stream = process.stdout
    offset = 0
    for revision in revisions:
        newline = stream.find(b"\n", offset)
        if newline == -1:
            blobs[revision] = None
            continue
        header = stream[offset:newline].decode("utf-8", "replace")
        offset = newline + 1
        parts = header.rsplit(" ", 2)
        # "<oid> missing" / "<oid> ambiguous" carry no payload.
        if len(parts) < 3 or not parts[2].isdigit():
            blobs[revision] = None
            continue
        size = int(parts[2])
        blobs[revision] = stream[offset : offset + size]
        offset += size + 1  # payload is followed by a trailing newline
    return blobs


def tree_paths(repo: Path, revision: str, profile: Profile) -> list[str]:
    """Supported, non-excluded paths in a revision's tree, sorted."""
    raw = git(repo, "ls-tree", "-r", "--name-only", "-z", revision)
    paths = [entry for entry in raw.split("\0") if entry]
    return sorted(
        path
        for path in paths
        if spec_for_extension(Path(path).suffix) is not None
        and not profile.is_excluded(path)
    )


def _chunks(items: list[str], size: int) -> Iterator[list[str]]:
    for start in range(0, len(items), size):
        yield items[start : start + size]


def scan_revision(
    repo: Path,
    revision: str,
    profile: Profile,
    jobs: int | None = None,
    cache=None,
) -> ScanReport:
    """Score every supported file in a revision's tree, without checking it out.

    This is the cache's best case: a revision's blobs are immutable, so a second
    gate run against the same baseline is served almost entirely from cache.
    """
    cache = cache if cache is not None else NullCache()
    paths = tree_paths(repo, revision, profile)
    reports: list[FileReport] = []

    # Blobs are read and analysed a chunk at a time so peak memory stays bounded
    # by the chunk rather than by the size of the repository.
    for chunk in _chunks(paths, BLOB_CHUNK_SIZE):
        blobs = read_blobs(repo, [f"{revision}:{path}" for path in chunk])
        work: list[AnalysisJob] = []
        for path in chunk:
            source = blobs.get(f"{revision}:{path}")
            if source is None:
                continue
            spec = spec_for_extension(Path(path).suffix)
            assert spec is not None  # tree_paths already filtered on this
            work.append((path, spec.name, source, profile))
        reports.extend(analyze_jobs(work, cache, jobs))

    scored = [report for report in reports if report.error is None]
    return ScanReport(
        version=__version__,
        profile=profile.name,
        score=aggregate_score(scored),
        loc=sum(report.loc for report in scored),
        files=tuple(reports),
    )
