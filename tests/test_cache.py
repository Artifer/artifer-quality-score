"""The report cache must be a pure speed optimisation.

The property that matters is that no cached run can ever differ from an uncached
one. A cache that changes an answer is worse than no cache.
"""

from __future__ import annotations

import sqlite3
from dataclasses import replace

from aqs import profile as prof
from aqs.analyze import scan
from aqs.cache import (
    CACHE_DIRNAME,
    NullCache,
    ReportCache,
    content_key,
    open_cache,
    profile_fingerprint,
)
from aqs.profile import DEFAULT_PROFILE, Profile

MESSY = """def f(a, b, c, d, e, f):
    total = 0
    if a:
        for i in range(b):
            while i:
                if i and a or b:
                    for j in range(i):
                        if j:
                            total += j
    return total
"""


def make_tree(root, count: int = 6):
    for index in range(count):
        (root / f"mod_{index}.py").write_text(MESSY)
    return root


class TestCorrectness:
    def test_cached_and_uncached_reports_are_identical(self, tmp_path):
        make_tree(tmp_path)
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            first = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            warm = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
        uncached = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path)

        assert first.model_dump_json() == uncached.model_dump_json()
        assert warm.model_dump_json() == uncached.model_dump_json()

    def test_second_run_hits_the_cache(self, tmp_path):
        make_tree(tmp_path)
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
            assert cache.hits == 0
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
            assert cache.hits == 6
            assert cache.misses == 0

    def test_editing_a_file_invalidates_only_that_file(self, tmp_path):
        make_tree(tmp_path)
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)

        (tmp_path / "mod_0.py").write_text("def clean(a: int) -> int:\n    return a\n")
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            report = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
            assert cache.hits == 5
            assert cache.misses == 1

        changed = next(f for f in report.files if f.path == "mod_0.py")
        assert changed.score == 10.0

    def test_changing_the_profile_invalidates_everything(self, tmp_path):
        """A score computed under different rules must never be reused."""
        make_tree(tmp_path)
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)

        stricter = Profile(
            name=DEFAULT_PROFILE.name,
            thresholds={**DEFAULT_PROFILE.thresholds,
                        "function_cyclomatic_complexity_warning": 3},
            rules=DEFAULT_PROFILE.rules,
            test_patterns=DEFAULT_PROFILE.test_patterns,
            exclude_patterns=DEFAULT_PROFILE.exclude_patterns,
            test_exempt_rules=DEFAULT_PROFILE.test_exempt_rules,
        )
        with open_cache(tmp_path, stricter) as cache:
            scan([tmp_path], stricter, relative_to=tmp_path, cache=cache)
            assert cache.hits == 0
            assert cache.misses == 6

    def test_same_bytes_at_a_different_path_is_a_separate_entry(self, tmp_path):
        """Path decides test-vs-production treatment, so it belongs in the key."""
        fingerprint = profile_fingerprint(DEFAULT_PROFILE)
        source = MESSY.encode()
        assert content_key("app.py", source, fingerprint) != content_key(
            "test_app.py", source, fingerprint
        )


class TestFingerprint:
    def test_is_stable_across_calls(self):
        assert profile_fingerprint(DEFAULT_PROFILE) == profile_fingerprint(
            DEFAULT_PROFILE
        )

    def test_changes_with_a_rule_weight(self):
        tweaked = Profile(
            name=DEFAULT_PROFILE.name,
            thresholds=DEFAULT_PROFILE.thresholds,
            rules={
                **DEFAULT_PROFILE.rules,
                prof.BUMPY_ROAD: replace(
                    DEFAULT_PROFILE.rules[prof.BUMPY_ROAD], weight=0.5
                ),
            },
            test_patterns=DEFAULT_PROFILE.test_patterns,
            exclude_patterns=DEFAULT_PROFILE.exclude_patterns,
            test_exempt_rules=DEFAULT_PROFILE.test_exempt_rules,
        )
        assert profile_fingerprint(tweaked) != profile_fingerprint(DEFAULT_PROFILE)

    def test_changes_with_exclusions(self):
        tweaked = Profile(
            name=DEFAULT_PROFILE.name,
            thresholds=DEFAULT_PROFILE.thresholds,
            rules=DEFAULT_PROFILE.rules,
            test_patterns=DEFAULT_PROFILE.test_patterns,
            exclude_patterns=("*/legacy/*",),
            test_exempt_rules=DEFAULT_PROFILE.test_exempt_rules,
        )
        assert profile_fingerprint(tweaked) != profile_fingerprint(DEFAULT_PROFILE)


class TestResilience:
    def test_disabled_cache_is_a_null_object(self, tmp_path):
        cache = open_cache(tmp_path, DEFAULT_PROFILE, enabled=False)
        assert isinstance(cache, NullCache)
        assert not cache.enabled
        assert not (tmp_path / CACHE_DIRNAME).exists()

    def test_unwritable_location_degrades_to_no_cache(self, tmp_path):
        """Being unable to write a cache is not a reason to refuse to score."""
        blocker = tmp_path / "blocked"
        blocker.write_text("I am a file, not a directory")
        cache = ReportCache(blocker / CACHE_DIRNAME, DEFAULT_PROFILE)
        assert not cache.enabled
        assert cache.get("anything") is None
        cache.put("anything", None)  # must not raise
        cache.close()

    def test_corrupt_entry_is_treated_as_a_miss(self, tmp_path):
        make_tree(tmp_path, count=1)
        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)

        database = tmp_path / CACHE_DIRNAME / "cache.sqlite3"
        with sqlite3.connect(database) as connection:
            connection.execute("UPDATE reports SET report = ?", ("{not valid json",))

        with open_cache(tmp_path, DEFAULT_PROFILE) as cache:
            report = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
            assert cache.misses == 1
        assert report.files[0].score < 10.0

    def test_scan_still_works_with_a_cache_that_cannot_open(self, tmp_path):
        make_tree(tmp_path, count=2)
        blocker = tmp_path / "blocked"
        blocker.write_text("not a directory")
        cache = ReportCache(blocker / CACHE_DIRNAME, DEFAULT_PROFILE)
        report = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, cache=cache)
        assert len(report.scored_files) == 2
