"""Delta against a git baseline — the behaviour the commit gate depends on."""

from __future__ import annotations

import subprocess

import pytest

from aqs import profile as prof
from aqs.delta import GitError, compute_delta, repo_root
from aqs.profile import DEFAULT_PROFILE

CLEAN = '''def add(a: int, b: int) -> int:
    """Add two numbers."""
    return a + b
'''

MESSY = '''def add(a, b, c, d, e, f):
    total = 0
    if a:
        for i in range(b):
            while i:
                if i and a or b:
                    for j in range(i):
                        if j:
                            total += j
    return total
'''


def git(repo, *args: str) -> None:
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True)


@pytest.fixture
def repo(tmp_path):
    """A git repo with one clean, committed Python file."""
    git(tmp_path, "init", "-q")
    git(tmp_path, "config", "user.email", "test@example.com")
    git(tmp_path, "config", "user.name", "Test")
    (tmp_path / "app.py").write_text(CLEAN)
    git(tmp_path, "add", "app.py")
    git(tmp_path, "commit", "-q", "-m", "clean")
    return tmp_path


class TestRegression:
    def test_making_a_file_worse_is_a_negative_delta(self, repo):
        (repo / "app.py").write_text(MESSY)
        report = compute_delta(repo, DEFAULT_PROFILE)

        assert report.score_change < 0
        assert len(report.files) == 1
        change = report.files[0]
        assert change.path == "app.py"
        assert change.old_score == 10.0
        assert change.new_score < 10.0
        assert change.is_regression
        introduced = {f.category for f in change.introduced}
        assert prof.DEEP_NESTED_COMPLEXITY in introduced
        assert change.fixed == ()

    def test_fixing_a_file_is_a_positive_delta(self, repo):
        (repo / "app.py").write_text(MESSY)
        git(repo, "add", "app.py")
        git(repo, "commit", "-q", "-m", "messy")
        (repo / "app.py").write_text(CLEAN)

        report = compute_delta(repo, DEFAULT_PROFILE)
        assert report.score_change > 0
        change = report.files[0]
        assert not change.is_regression
        assert {f.category for f in change.fixed} >= {prof.DEEP_NESTED_COMPLEXITY}
        assert change.introduced == ()

    def test_degraded_reports_a_worsening_of_an_existing_smell(self, repo):
        few = "\n".join(f"    if x == {i}: return {i}" for i in range(14))
        many = "\n".join(f"    if x == {i}: return {i}" for i in range(60))
        (repo / "app.py").write_text(f"def f(x):\n{few}\n")
        git(repo, "add", "app.py")
        git(repo, "commit", "-q", "-m", "complex")
        (repo / "app.py").write_text(f"def f(x):\n{many}\n")

        change = compute_delta(repo, DEFAULT_PROFILE).files[0]
        assert {f.category for f in change.degraded} == {prof.COMPLEX_METHOD}
        # Growing past 60 lines also trips Large Method, which is genuinely new;
        # the point is that the pre-existing Complex Method is not re-reported.
        assert prof.COMPLEX_METHOD not in {f.category for f in change.introduced}

    def test_no_changes_is_a_zero_delta(self, repo):
        report = compute_delta(repo, DEFAULT_PROFILE)
        assert report.files == ()
        assert report.score_change == 0.0


class TestUntrackedAndDeleted:
    def test_untracked_file_is_scored_against_a_clean_slate(self, repo):
        """Freshly generated code has no baseline, and is exactly what to check."""
        (repo / "new.py").write_text(MESSY)
        report = compute_delta(repo, DEFAULT_PROFILE)
        paths = {f.path for f in report.files}
        assert "new.py" in paths
        change = next(f for f in report.files if f.path == "new.py")
        assert change.old_score is None
        assert change.score_change < 0

    def test_deleting_a_bad_file_is_an_improvement(self, repo):
        (repo / "bad.py").write_text(MESSY)
        git(repo, "add", "bad.py")
        git(repo, "commit", "-q", "-m", "bad")
        (repo / "bad.py").unlink()

        report = compute_delta(repo, DEFAULT_PROFILE)
        change = next(f for f in report.files if f.path == "bad.py")
        assert change.new_score is None
        assert change.score_change > 0


class TestStaged:
    def test_staged_mode_ignores_unstaged_edits(self, repo):
        (repo / "app.py").write_text(MESSY)
        git(repo, "add", "app.py")
        # A further unstaged edit that must not be scored.
        (repo / "app.py").write_text(MESSY + "\n\nx = 1\n")

        staged = compute_delta(repo, DEFAULT_PROFILE, staged=True)
        assert len(staged.files) == 1
        assert staged.files[0].new_score < 10.0


class TestMatching:
    def test_findings_match_by_function_not_line_number(self, repo):
        """Inserting a line above a function must not look like a new smell."""
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(14))
        source = f"def f(x):\n{branches}\n"
        (repo / "app.py").write_text(source)
        git(repo, "add", "app.py")
        git(repo, "commit", "-q", "-m", "complex")

        (repo / "app.py").write_text("# a new comment\n\n" + source)
        change = compute_delta(repo, DEFAULT_PROFILE).files[0]
        assert change.introduced == ()
        assert change.degraded == ()
        assert change.score_change == 0.0


class TestErrors:
    def test_outside_a_repository_raises(self, tmp_path):
        with pytest.raises(GitError):
            repo_root(tmp_path)

    def test_unknown_ref_raises(self, repo):
        with pytest.raises(GitError):
            compute_delta(repo, DEFAULT_PROFILE, baseline="no-such-ref")
