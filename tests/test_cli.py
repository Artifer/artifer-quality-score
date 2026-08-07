"""CLI contract: output shapes and exit codes, which are what automation binds to."""

from __future__ import annotations

import json
import subprocess

import pytest
from typer.testing import CliRunner

from aqs import profile as prof
from aqs.cli import EXIT_ERROR, EXIT_GATE_FAILED, EXIT_OK, app

runner = CliRunner()

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


@pytest.fixture
def tree(tmp_path):
    (tmp_path / "clean.py").write_text(CLEAN)
    (tmp_path / "messy.py").write_text(MESSY)
    return tmp_path


class TestScan:
    def test_reports_and_exits_zero(self, tree):
        result = runner.invoke(app, ["scan", str(tree)])
        assert result.exit_code == EXIT_OK
        assert "Artifer Quality Score" in result.stdout
        assert "messy.py" in result.stdout

    def test_clean_tree_says_so(self, tmp_path):
        (tmp_path / "clean.py").write_text(CLEAN)
        result = runner.invoke(app, ["scan", str(tmp_path)])
        assert result.exit_code == EXIT_OK
        assert "No findings" in result.stdout

    def test_json_output_is_parseable_and_complete(self, tree):
        result = runner.invoke(app, ["scan", str(tree), "--json"])
        assert result.exit_code == EXIT_OK
        payload = json.loads(result.stdout)
        assert set(payload) >= {"version", "profile", "score", "loc", "files"}
        assert 1.0 <= payload["score"] <= 10.0
        messy = next(f for f in payload["files"] if f["path"] == "messy.py")
        assert messy["score"] < 10.0
        smell = messy["function_level_code_smells"][0]["code_smells"][0]
        assert set(smell) >= {"category", "details", "penalty", "highlight_range"}

    def test_guidance_covers_every_reported_rule(self, tree):
        """The payload a model receives must explain every rule it is charged for."""
        result = runner.invoke(app, ["scan", str(tree), "--json", "--guidance"])
        payload = json.loads(result.stdout)
        reported = {
            smell["category"]
            for f in payload["files"]
            for group in f["function_level_code_smells"]
            for smell in group["code_smells"]
        } | {
            smell["category"]
            for f in payload["files"]
            for smell in f["file_level_code_smells"]
        }
        assert reported
        assert reported <= set(payload["guidance"])
        for entry in payload["guidance"].values():
            assert set(entry) == {"summary", "why", "fix"}

    def test_min_score_gate_fails(self, tree):
        result = runner.invoke(app, ["scan", str(tree), "--min-score", "9.9"])
        assert result.exit_code == EXIT_GATE_FAILED

    def test_min_score_gate_passes(self, tree):
        result = runner.invoke(app, ["scan", str(tree), "--min-score", "1.0"])
        assert result.exit_code == EXIT_OK

    def test_missing_path_is_an_error(self, tmp_path):
        result = runner.invoke(app, ["scan", str(tmp_path / "nope")])
        assert result.exit_code == EXIT_ERROR


class TestCheck:
    def test_emits_lint_style_lines(self, tree):
        result = runner.invoke(app, ["check", str(tree)])
        assert result.exit_code == EXIT_OK
        lines = [line for line in result.stdout.splitlines() if line.strip()]
        assert lines
        for line in lines:
            head, _, rest = line.partition(": ")
            path, line_no, column = head.rsplit(":", 2)
            assert path.endswith(".py")
            assert line_no.isdigit() and column.isdigit()
            assert rest

    def test_fail_on_findings_gate(self, tree):
        result = runner.invoke(app, ["check", str(tree), "--fail-on-findings"])
        assert result.exit_code == EXIT_GATE_FAILED

    def test_clean_tree_passes_the_gate(self, tmp_path):
        (tmp_path / "clean.py").write_text(CLEAN)
        result = runner.invoke(app, ["check", str(tmp_path), "--fail-on-findings"])
        assert result.exit_code == EXIT_OK


class TestExplain:
    def test_explains_a_rule(self):
        result = runner.invoke(app, ["explain", "Bumpy Road Ahead"])
        assert result.exit_code == EXIT_OK
        assert "How to fix it" in result.stdout

    def test_fuzzy_name_resolves(self):
        result = runner.invoke(app, ["explain", "bumpy"])
        assert result.exit_code == EXIT_OK
        assert "Bumpy Road Ahead" in result.stdout

    def test_unknown_rule_errors(self):
        result = runner.invoke(app, ["explain", "Not A Rule"])
        assert result.exit_code == EXIT_ERROR

    def test_list_covers_every_rule(self):
        result = runner.invoke(app, ["explain", "--list", "--json"])
        assert result.exit_code == EXIT_OK
        payload = json.loads(result.stdout)
        from aqs.explain import RULE_DOCS

        assert set(payload) == set(RULE_DOCS)
        assert set(payload) == set(prof.DEFAULT_RULES)

    def test_thresholds_listing(self):
        result = runner.invoke(app, ["explain", "--thresholds", "--json"])
        payload = json.loads(result.stdout)
        assert payload == prof.DEFAULT_THRESHOLDS


class TestDeltaCommand:
    @pytest.fixture
    def repo(self, tmp_path):
        def git(*args):
            subprocess.run(["git", *args], cwd=tmp_path, check=True, capture_output=True)

        git("init", "-q")
        git("config", "user.email", "t@example.com")
        git("config", "user.name", "T")
        (tmp_path / "app.py").write_text(CLEAN)
        git("add", "app.py")
        git("commit", "-q", "-m", "clean")
        return tmp_path

    def test_regression_fails_the_gate(self, repo):
        (repo / "app.py").write_text(MESSY)
        result = runner.invoke(app, ["delta", "--path", str(repo)])
        assert result.exit_code == EXIT_GATE_FAILED
        # The change-type label must survive an 80-column terminal.
        assert "new" in result.stdout
        assert "Regression" in result.stdout

    def test_max_drop_tolerance_allows_a_small_regression(self, repo):
        (repo / "app.py").write_text(MESSY)
        result = runner.invoke(
            app, ["delta", "--path", str(repo), "--max-drop", "10"]
        )
        assert result.exit_code == EXIT_OK

    def test_no_changes_passes(self, repo):
        result = runner.invoke(app, ["delta", "--path", str(repo)])
        assert result.exit_code == EXIT_OK
        assert "No changed files" in result.stdout

    def test_json_output(self, repo):
        (repo / "app.py").write_text(MESSY)
        result = runner.invoke(app, ["delta", "--path", str(repo), "--json"])
        payload = json.loads(result.stdout)
        assert set(payload) >= {"baseline", "old_score", "new_score", "score_change"}
        assert payload["score_change"] < 0

    def test_outside_a_repository_is_an_error(self, tmp_path):
        result = runner.invoke(app, ["delta", "--path", str(tmp_path)])
        assert result.exit_code == EXIT_ERROR


class TestConfigErrors:
    def test_bad_config_is_reported_not_ignored(self, tmp_path):
        (tmp_path / ".aqs.json").write_text(json.dumps({"rules": {"Nope": 1}}))
        (tmp_path / "a.py").write_text(CLEAN)
        result = runner.invoke(app, ["scan", str(tmp_path)])
        assert result.exit_code == EXIT_ERROR
