"""Content-addressed report cache.

A file's report is a pure function of (file bytes, profile, tool version). Nothing
else — no history, no clock, no neighbours. That makes it trivially cacheable, and
caching is where the real speed is: the baseline side of a merge gate is an
immutable commit, so after the first run every one of its files is a cache hit, and
the current side only re-analyses what actually changed.

Lookups and writes happen in the parent process only. Workers stay pure functions
of their input, so there is no sqlite concurrency to reason about.

The key includes the tool version and a fingerprint of the profile, so bumping
either invalidates everything rather than silently mixing scores computed under
different rules.
"""

from __future__ import annotations

import hashlib
import json
import sqlite3
from dataclasses import asdict
from pathlib import Path

from .models import FileReport
from .profile import Profile
from .version import __version__

CACHE_DIRNAME = ".aqs"
CACHE_FILENAME = "cache.sqlite3"

_SCHEMA = """
CREATE TABLE IF NOT EXISTS reports (
    key TEXT PRIMARY KEY,
    report TEXT NOT NULL
) WITHOUT ROWID;
"""


def profile_fingerprint(profile: Profile) -> str:
    """A stable hash of everything in a profile that can change a score."""
    payload = json.dumps(
        {
            "name": profile.name,
            "thresholds": profile.thresholds,
            "rules": {name: asdict(rule) for name, rule in profile.rules.items()},
            "test_patterns": sorted(profile.test_patterns),
            "exclude_patterns": sorted(profile.exclude_patterns),
            "test_exempt_rules": sorted(profile.test_exempt_rules),
        },
        sort_keys=True,
    )
    return hashlib.sha256(payload.encode()).hexdigest()[:16]


def content_key(path: str, source: bytes, fingerprint: str) -> str:
    """Cache key for one file.

    The path is part of the key because it decides test-vs-production treatment
    and is echoed in the report, so the same bytes at two paths are two reports.
    """
    digest = hashlib.sha256()
    digest.update(__version__.encode())
    digest.update(b"\0")
    digest.update(fingerprint.encode())
    digest.update(b"\0")
    digest.update(path.encode())
    digest.update(b"\0")
    digest.update(source)
    return digest.hexdigest()


class ReportCache:
    """A sqlite-backed store of scored file reports.

    Failures are never fatal: a cache that cannot be opened or written degrades to
    no caching at all. Being unable to write a cache file is not a reason to refuse
    to score someone's code.
    """

    def __init__(self, directory: Path, profile: Profile) -> None:
        self.fingerprint = profile_fingerprint(profile)
        self._pending: dict[str, str] = {}
        self._connection: sqlite3.Connection | None = None
        self.hits = 0
        self.misses = 0
        try:
            directory.mkdir(parents=True, exist_ok=True)
            self._connection = sqlite3.connect(directory / CACHE_FILENAME)
            self._connection.executescript(_SCHEMA)
        except (OSError, sqlite3.Error):
            self._connection = None

    @property
    def enabled(self) -> bool:
        return self._connection is not None

    def get(self, key: str) -> FileReport | None:
        if self._connection is None:
            return None
        try:
            row = self._connection.execute(
                "SELECT report FROM reports WHERE key = ?", (key,)
            ).fetchone()
        except sqlite3.Error:
            return None
        if row is None:
            self.misses += 1
            return None
        try:
            report = FileReport.model_validate_json(row[0])
        except ValueError:
            # A report written by an incompatible schema; treat as a miss.
            self.misses += 1
            return None
        self.hits += 1
        return report

    def put(self, key: str, report: FileReport) -> None:
        if self._connection is None:
            return
        self._pending[key] = report.model_dump_json()

    def commit(self) -> None:
        """Flush pending writes in one transaction."""
        if self._connection is None or not self._pending:
            return
        try:
            with self._connection:
                self._connection.executemany(
                    "INSERT OR REPLACE INTO reports (key, report) VALUES (?, ?)",
                    list(self._pending.items()),
                )
        except sqlite3.Error:
            pass
        finally:
            self._pending.clear()

    def close(self) -> None:
        self.commit()
        if self._connection is not None:
            try:
                self._connection.close()
            except sqlite3.Error:
                pass
            self._connection = None

    def __enter__(self) -> "ReportCache":
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()


class NullCache:
    """Stand-in used when caching is switched off, so callers need no branches."""

    enabled = False
    hits = 0
    misses = 0
    fingerprint = ""

    def get(self, key: str) -> FileReport | None:
        return None

    def put(self, key: str, report: FileReport) -> None:
        return None

    def commit(self) -> None:
        return None

    def close(self) -> None:
        return None

    def __enter__(self) -> "NullCache":
        return self

    def __exit__(self, *exc_info: object) -> None:
        return None


def open_cache(
    root: Path | None, profile: Profile, enabled: bool = True
) -> ReportCache | NullCache:
    if not enabled or root is None:
        return NullCache()
    return ReportCache(root / CACHE_DIRNAME, profile)
