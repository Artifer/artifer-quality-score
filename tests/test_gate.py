"""The merge gate: whole-repository, no-regression enforcement."""

from __future__ import annotations

import subprocess

import pytest

from aqs.gate import compute_gate
from aqs.gitio import GitError, scan_revision, tree_paths
from aqs.profile import DEFAULT_PROFILE

CLEAN = 'def add(a: int, b: int) -> int:\n    """Add."""\n    return a + b\n'
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
#: Individually acceptable, but below a pristine repo's average, and big enough
#: to shift the LoC weights on its own.
MEDIOCRE = "def g(x):\n" + "\n".join(
    f"    if x == {i}: return {i}" for i in range(14)
) + "\n" + "\n".join(f"y{i} = {i}" for i in range(120)) + "\n"


def git(repo, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)


def commit(repo, message: str) -> None:
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", message)


@pytest.fixture
def repo(tmp_path):
    """A repo on `develop` with several clean committed files."""
    git(tmp_path, "init", "-q", "-b", "develop")
    git(tmp_path, "config", "user.email", "test@example.com")
    git(tmp_path, "config", "user.name", "Test")
    for index in range(4):
        (tmp_path / f"mod_{index}.py").write_text(CLEAN)
    commit(tmp_path, "clean baseline")
    return tmp_path


class TestNoRegressionRule:
    def test_unchanged_repository_passes(self, repo):
        report = compute_gate(repo, DEFAULT_PROFILE, against="develop")
        assert report.passed
        assert report.score_change == 0.0
        assert report.baseline_score == report.current_score == 10.0

    def test_degrading_a_file_fails(self, repo):
        git(repo, "checkout", "-q", "-b", "feature")
        (repo / "mod_0.py").write_text(MESSY)
        commit(repo, "make it worse")

        report = compute_gate(repo, DEFAULT_PROFILE, against="develop")
        assert not report.passed
        assert report.score_change < 0
        assert [f.path for f in report.regressions] == ["mod_0.py"]

    def test_improving_a_file_passes(self, repo):
        # Land the mess on develop itself, then branch off to clean it up.
        (repo / "mod_0.py").write_text(MESSY)
        commit(repo, "messy")
        git(repo, "checkout", "-q", "-b", "cleanup")
        (repo / "mod_0.py").write_text(CLEAN)
        commit(repo, "clean it up")

        report = compute_gate(repo, DEFAULT_PROFILE, against="develop")
        assert report.passed
        assert report.score_change > 0

    def test_tolerance_permits_a_small_drop(self, repo):
        git(repo, "checkout", "-q", "-b", "feature")
        (repo / "mod_0.py").write_text(MESSY)
        commit(repo, "worse")

        strict = compute_gate(repo, DEFAULT_PROFILE, against="develop", tolerance=0.0)
        lenient = compute_gate(repo, DEFAULT_PROFILE, against="develop", tolerance=10.0)
        assert not strict.passed
        assert lenient.passed


class TestWholeTreeIsNecessary:
    def test_catches_weight_shift_that_delta_cannot_see(self, repo):
        """The reason the gate scans everything.

        Adding a large, individually-acceptable file lowers a LoC-weighted average
        without any existing file regressing. A changed-files comparison reports
        nothing wrong; the repository score still falls.
        """
        from aqs.delta import compute_delta

        git(repo, "checkout", "-q", "-b", "feature")
        (repo / "new_module.py").write_text(MEDIOCRE)
        commit(repo, "add a big mediocre module")

        gate = compute_gate(repo, DEFAULT_PROFILE, against="develop")
        assert not gate.passed
        assert gate.score_change < 0
        # No pre-existing file got worse — the drop is purely weight shift.
        assert gate.regressions == () or all(
            f.path == "new_module.py" for f in gate.regressions
        )

        # The changed-files view sees only the new file scored against a clean
        # slate, and reports a far smaller change than the repository actually took.
        delta = compute_delta(repo, DEFAULT_PROFILE, baseline="develop")
        assert delta.score_change != gate.score_change


class TestMergeBase:
    def test_merge_base_ignores_other_work_on_the_target(self, repo):
        """Your branch must not be failed for someone else's regression."""
        git(repo, "checkout", "-q", "-b", "feature")
        (repo / "mine.py").write_text(CLEAN)
        commit(repo, "my clean addition")

        # Meanwhile, someone lands a mess on develop.
        git(repo, "checkout", "-q", "develop")
        (repo / "theirs.py").write_text(MESSY)
        commit(repo, "their mess")
        git(repo, "checkout", "-q", "feature")

        with_merge_base = compute_gate(
            repo, DEFAULT_PROFILE, against="develop", merge_base=True
        )
        assert with_merge_base.passed

        against_tip = compute_gate(
            repo, DEFAULT_PROFILE, against="develop", merge_base=False
        )
        # Compared against the tip, their mess is missing from our tree, so the
        # comparison is not about our change at all.
        assert against_tip.score_change != with_merge_base.score_change

    def test_baseline_is_recorded_as_a_concrete_sha(self, repo):
        report = compute_gate(
            repo, DEFAULT_PROFILE, against="develop", merge_base=False
        )
        assert len(report.baseline_revision) == 40
        assert all(c in "0123456789abcdef" for c in report.baseline_revision)

    def test_unrelated_ref_raises(self, repo):
        with pytest.raises(GitError):
            compute_gate(repo, DEFAULT_PROFILE, against="no-such-branch")


class TestScanRevision:
    def test_scores_a_revision_without_checking_it_out(self, repo):
        (repo / "mod_0.py").write_text(MESSY)
        commit(repo, "messy")

        head = scan_revision(repo, "HEAD", DEFAULT_PROFILE)
        previous = scan_revision(repo, "HEAD~1", DEFAULT_PROFILE)
        assert previous.score == 10.0
        assert head.score < 10.0
        # The working tree is untouched by scoring history.
        assert (repo / "mod_0.py").read_text() == MESSY

    def test_respects_exclusions(self, repo):
        (repo / "node_modules").mkdir()
        (repo / "node_modules" / "junk.js").write_text(MESSY)
        commit(repo, "vendored")
        assert "node_modules/junk.js" not in tree_paths(
            repo, "HEAD", DEFAULT_PROFILE
        )

    def test_matches_a_filesystem_scan_of_the_same_tree(self, repo):
        """Scoring from git blobs must agree with scoring from disk."""
        from aqs.analyze import scan

        (repo / "mod_0.py").write_text(MESSY)
        commit(repo, "messy")

        from_git = scan_revision(repo, "HEAD", DEFAULT_PROFILE)
        from_disk = scan([repo], DEFAULT_PROFILE, relative_to=repo)
        assert from_git.score == from_disk.score
        assert from_git.loc == from_disk.loc
