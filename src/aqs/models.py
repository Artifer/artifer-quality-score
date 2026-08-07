"""Report data model.

The structure mirrors CodeScene's `cs review` output (file-level smells,
function-level smells grouped per function, a 1.0-10.0 score) because that shape
has already been through real-world iteration. Key names are snake_case rather
than their kebab-case, since everything consuming this is ours.

Unlike `cs review`, every finding carries its own `penalty`. That is the point of
the tool: a score with no visible derivation is not something you can optimise
against.
"""

from __future__ import annotations

from pydantic import BaseModel, ConfigDict, Field

SCORE_MAX = 10.0
SCORE_MIN = 1.0


class Range(BaseModel):
    """1-indexed, inclusive source range."""

    model_config = ConfigDict(frozen=True)

    start_line: int
    start_column: int = 1
    end_line: int
    end_column: int = 1

    def __str__(self) -> str:
        if self.start_line == self.end_line:
            return str(self.start_line)
        return f"{self.start_line}-{self.end_line}"


class Finding(BaseModel):
    """One code smell at one location."""

    model_config = ConfigDict(frozen=True)

    category: str
    #: Human-readable measurement, e.g. "cc = 14".
    details: str
    highlight_range: Range
    #: Points deducted from this file's score, after rule weighting.
    penalty: float
    #: The measured value the rule fired on, for machine consumers.
    value: float | None = None
    #: The configured threshold it exceeded.
    threshold: float | None = None
    #: Enclosing function, when the smell is function- or expression-level.
    function: str | None = None


class FunctionReport(BaseModel):
    model_config = ConfigDict(frozen=True)

    function: str
    range: Range
    code_smells: tuple[Finding, ...] = ()

    @property
    def penalty(self) -> float:
        return sum(smell.penalty for smell in self.code_smells)


class FileReport(BaseModel):
    model_config = ConfigDict(frozen=True)

    path: str
    language: str
    #: Non-blank, non-comment lines.
    loc: int
    score: float
    file_level_code_smells: tuple[Finding, ...] = ()
    function_level_code_smells: tuple[FunctionReport, ...] = ()
    #: Set when the file could not be read or has no supported language, in which
    #: case it is excluded from aggregation entirely.
    error: str | None = None
    #: Count of syntax errors tree-sitter recovered from. The file is still
    #: scored — the parser recovers well and partial trees still measure most
    #: things — but a non-zero count means treat the number with suspicion.
    syntax_errors: int = 0

    @property
    def all_findings(self) -> tuple[Finding, ...]:
        nested = tuple(
            smell
            for fn in self.function_level_code_smells
            for smell in fn.code_smells
        )
        return self.file_level_code_smells + nested

    @property
    def total_penalty(self) -> float:
        return sum(finding.penalty for finding in self.all_findings)


class ScanReport(BaseModel):
    model_config = ConfigDict(frozen=True)

    #: Tool version, so a score can always be traced to the code that produced it.
    version: str
    #: Name of the scoring profile in force.
    profile: str
    #: LoC-weighted mean of file scores.
    score: float
    loc: int
    files: tuple[FileReport, ...] = ()

    @property
    def findings_count(self) -> int:
        return sum(len(f.all_findings) for f in self.files)

    @property
    def scored_files(self) -> tuple[FileReport, ...]:
        return tuple(f for f in self.files if f.error is None)


class FileDelta(BaseModel):
    """Score change for a single file between a baseline and the working tree."""

    model_config = ConfigDict(frozen=True)

    path: str
    old_score: float | None
    new_score: float | None
    score_change: float
    #: Smells present now but not in the baseline.
    introduced: tuple[Finding, ...] = ()
    #: Smells present in the baseline but not now.
    fixed: tuple[Finding, ...] = ()
    #: Smells present in both, but measurably worse now.
    degraded: tuple[Finding, ...] = ()
    #: Smells present in both, but measurably better now.
    improved: tuple[Finding, ...] = ()

    @property
    def is_regression(self) -> bool:
        return self.score_change < 0


class DeltaReport(BaseModel):
    model_config = ConfigDict(frozen=True)

    version: str
    profile: str
    baseline: str
    old_score: float
    new_score: float
    score_change: float
    files: tuple[FileDelta, ...] = ()

    @property
    def regressions(self) -> tuple[FileDelta, ...]:
        return tuple(f for f in self.files if f.score_change < 0)

    @property
    def improvements(self) -> tuple[FileDelta, ...]:
        return tuple(f for f in self.files if f.score_change > 0)


class GateReport(BaseModel):
    """Whole-repository comparison between a baseline revision and now.

    Distinct from `DeltaReport`: these scores are LoC-weighted over *every* file
    in each tree, not just the changed ones. That is what makes it able to catch a
    regression caused purely by weight shift — a new large file that is
    individually fine but below the repo's average.
    """

    model_config = ConfigDict(frozen=True)

    version: str
    profile: str
    #: The ref the user asked to compare against, e.g. "origin/develop".
    against: str
    #: The commit actually used — the merge base, unless disabled.
    baseline_revision: str
    baseline_score: float
    baseline_loc: int
    baseline_file_count: int
    current_score: float
    current_loc: int
    current_file_count: int
    score_change: float
    #: Allowed drop before the gate fails.
    tolerance: float
    #: Per-file detail for the changed files, so a failure is actionable.
    files: tuple[FileDelta, ...] = ()

    @property
    def passed(self) -> bool:
        return self.score_change >= -abs(self.tolerance)

    @property
    def regressions(self) -> tuple[FileDelta, ...]:
        return tuple(f for f in self.files if f.score_change < 0)
