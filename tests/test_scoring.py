"""Scoring arithmetic, caps, and the determinism guarantee."""

from __future__ import annotations

from dataclasses import replace

from aqs import profile as prof
from aqs.models import SCORE_MAX, SCORE_MIN, FileReport
from aqs.profile import DEFAULT_PROFILE, Profile, RuleConfig
from aqs.scoring import aggregate_score

CLEAN = '''
def add(a: int, b: int) -> int:
    """Add two numbers."""
    return a + b
'''


class TestFileScore:
    def test_clean_code_scores_ten(self, score):
        report = score(CLEAN)
        assert report.score == SCORE_MAX
        assert report.all_findings == ()

    def test_penalties_sum_to_the_deduction(self, score):
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(14))
        report = score(f"def f(x):\n{branches}\n")
        expected = SCORE_MAX - sum(f.penalty for f in report.all_findings)
        assert report.score == round(expected, 4)

    def test_score_floors_at_one(self, score):
        """A truly awful file bottoms out rather than going negative."""
        chunk = (
            "    if a:\n"
            "        for b in a:\n"
            "            while b:\n"
            "                if b and a or b:\n"
            "                    for c in b:\n"
            "                        if c:\n"
            "                            a += c\n"
        )
        source = "".join(
            f"def f{i}(a, b, c, d, e, f):\n{chunk * 4}\n" for i in range(20)
        )
        report = score(source)
        assert report.score == SCORE_MIN

    def test_findings_are_ordered_by_position(self, score):
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(14))
        report = score(f"def a(x):\n{branches}\n\n\ndef b(x):\n{branches}\n")
        lines = [f.highlight_range.start_line for f in report.all_findings]
        assert lines == sorted(lines)


class TestCaps:
    def test_per_finding_cap_is_respected(self, score):
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(200))
        report = score(f"def f(x):\n{branches}\n")
        complex_findings = [
            f for f in report.all_findings if f.category == prof.COMPLEX_METHOD
        ]
        cap = DEFAULT_PROFILE.rule(prof.COMPLEX_METHOD).cap
        assert complex_findings[0].penalty <= cap

    def test_per_file_cap_limits_one_rule(self, score):
        """Forty mediocre functions must not consume the whole budget."""
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(12))
        source = "\n\n".join(f"def f{i}(x):\n{branches}" for i in range(40))
        report = score(source)
        spent = sum(
            f.penalty for f in report.all_findings if f.category == prof.COMPLEX_METHOD
        )
        assert spent <= DEFAULT_PROFILE.rule(prof.COMPLEX_METHOD).file_cap + 1e-6

    def test_worst_offender_keeps_full_penalty_when_cap_binds(self, score):
        """Fixing the worst function must always move the score."""
        mild = "\n".join(f"    if x == {i}: return {i}" for i in range(12))
        severe = "\n".join(f"    if x == {i}: return {i}" for i in range(120))
        source = "\n\n".join(f"def mild{i}(x):\n{mild}" for i in range(30))
        source += f"\n\n\ndef severe(x):\n{severe}\n"
        report = score(source)
        by_function = {
            f.function: f.penalty
            for f in report.all_findings
            if f.category == prof.COMPLEX_METHOD
        }
        assert by_function["severe"] == DEFAULT_PROFILE.rule(prof.COMPLEX_METHOD).cap


class TestRuleConfig:
    def test_penalty_grows_past_the_threshold(self):
        rule = RuleConfig(base=1.0, scale=10, cap=5.0)
        assert rule.penalty(10, 10) == 1.0
        assert rule.penalty(20, 10) == 2.0
        assert rule.penalty(30, 10) == 3.0

    def test_penalty_is_capped(self):
        rule = RuleConfig(base=1.0, scale=10, cap=2.5)
        assert rule.penalty(1000, 10) == 2.5

    def test_zero_weight_is_free(self):
        rule = RuleConfig(base=1.0, scale=10, cap=5.0, weight=0.0)
        assert rule.penalty(100, 10) == 0.0

    def test_weight_scales_linearly(self):
        rule = RuleConfig(base=1.0, scale=10, cap=5.0, weight=0.5)
        assert rule.penalty(10, 10) == 0.5


class TestAggregation:
    def _report(self, score: float, loc: int, path: str) -> FileReport:
        return FileReport(path=path, language="python", loc=loc, score=score)

    def test_weighted_by_lines_of_code(self):
        """CodeScene's published example: a big bad file must dominate.

        Their documentation gives 2.0@5000 LoC, 10.0@100 and 10.0@10 as an
        illustration of why the naive mean (7.33) is misleading.
        """
        files = [
            self._report(2.0, 5000, "big.py"),
            self._report(10.0, 100, "small.py"),
            self._report(10.0, 10, "tiny.py"),
        ]
        naive = sum(f.score for f in files) / len(files)
        assert round(naive, 2) == 7.33
        assert aggregate_score(files) == round(11100 / 5110, 4)
        assert aggregate_score(files) < 2.2

    def test_empty_input_is_perfect_not_zero(self):
        assert aggregate_score([]) == SCORE_MAX

    def test_unparsed_files_are_excluded(self):
        files = [
            self._report(2.0, 100, "bad.py"),
            FileReport(
                path="x.rs", language="unknown", loc=0, score=10.0, error="unsupported"
            ),
        ]
        assert aggregate_score(files) == 2.0


class TestDeterminism:
    def test_repeated_analysis_is_byte_identical(self, score):
        """The whole gate depends on this: same bytes in, same JSON out."""
        source = "def f(a, b, c, d, e):\n    if a and b and c:\n        return 1\n"
        first = score(source).model_dump_json()
        second = score(source).model_dump_json()
        assert first == second

    def test_score_does_not_depend_on_file_name(self, score):
        source = "def f(a, b, c, d, e):\n    return a\n"
        assert score(source, path="a.py").score == score(source, path="z.py").score

    def test_scan_order_is_stable(self, tmp_path):
        from aqs.analyze import scan

        for name in ("z.py", "a.py", "m.py"):
            (tmp_path / name).write_text("def f(a, b, c, d, e):\n    return a\n")
        first = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path)
        second = scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path)
        assert [f.path for f in first.files] == [f.path for f in second.files]
        assert [f.path for f in first.files] == ["a.py", "m.py", "z.py"]

    def test_parallel_and_serial_agree_exactly(self, tmp_path, monkeypatch):
        """Parallelism must never be observable in the output."""
        from aqs import analyze

        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(14))
        for index in range(8):
            (tmp_path / f"mod_{index}.py").write_text(
                f"def f{index}(x, a, b, c, d):\n{branches}\n"
            )

        serial = analyze.scan([tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, jobs=1)
        monkeypatch.setattr(analyze, "PARALLEL_FILE_THRESHOLD", 2)
        parallel = analyze.scan(
            [tmp_path], DEFAULT_PROFILE, relative_to=tmp_path, jobs=4
        )
        assert serial.model_dump_json() == parallel.model_dump_json()


class TestWeightDisabling:
    def test_disabled_rule_produces_no_findings(self, score):
        custom = Profile(
            name="no-complexity",
            thresholds=dict(DEFAULT_PROFILE.thresholds),
            rules={
                **DEFAULT_PROFILE.rules,
                prof.COMPLEX_METHOD: replace(
                    DEFAULT_PROFILE.rules[prof.COMPLEX_METHOD], weight=0.0
                ),
            },
            test_patterns=DEFAULT_PROFILE.test_patterns,
            exclude_patterns=DEFAULT_PROFILE.exclude_patterns,
            test_exempt_rules=DEFAULT_PROFILE.test_exempt_rules,
        )
        branches = "\n".join(f"    if x == {i}: return {i}" for i in range(14))
        report = score(f"def f(x):\n{branches}\n", custom_profile=custom)
        assert prof.COMPLEX_METHOD not in {f.category for f in report.all_findings}
