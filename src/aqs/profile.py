"""Thresholds and penalty weights — the entire scoring policy, in one place.

The rule *taxonomy* here follows CodeScene's published code-health factors, which
are well documented and research-backed. The *weights* are ours: CodeScene's are
calibrated against proprietary baseline data and have been recalibrated several
times across their releases, so there is no public number to copy. Ours are
tuned by hand against the Artifer codebases (see README) and are deliberately
easy to change.

Threshold keys reuse CodeScene's public naming where a published equivalent
exists (`function_cyclomatic_complexity_warning`, `function_nesting_depth_warning`,
`constructor_max_arguments`, `function_duplication_min_lines_of_code_for_check`,
`function_duplication_min_similarity_percentage`) so that anyone who has written
a `code-health-rules.json` finds the names familiar.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, replace
from fnmatch import fnmatch
from pathlib import Path
from typing import Any

# Rule identifiers. These strings are the public contract: they appear in JSON
# output, in `aqs explain`, and as keys in user config.
COMPLEX_METHOD = "Complex Method"
LARGE_METHOD = "Large Method"
DEEP_NESTED_COMPLEXITY = "Deep, Nested Complexity"
BUMPY_ROAD = "Bumpy Road Ahead"
COMPLEX_CONDITIONAL = "Complex Conditional"
EXCESS_ARGUMENTS = "Excess Number of Function Arguments"
BRAIN_METHOD = "Brain Method"
CONSTRUCTOR_OVER_INJECTION = "Constructor Over-Injection"
STRING_HEAVY_ARGUMENTS = "String Heavy Function Arguments"
PRIMITIVE_OBSESSION = "Primitive Obsession"
CODE_DUPLICATION = "Code Duplication"
LARGE_ASSERTION_BLOCKS = "Large Assertion Blocks"
DUPLICATED_ASSERTION_BLOCKS = "Duplicated Assertion Blocks"
FILE_SIZE = "Lines of Code in a Single File"
DECLARATION_SIZE = "Lines of Declarations in a Single File"
FUNCTION_COUNT = "Number of Functions in a Single Module"
OVERALL_COMPLEXITY = "Overall Code Complexity"
BRAIN_CLASS = "Brain Class"
LOW_COHESION = "Low Cohesion"


@dataclass(frozen=True)
class RuleConfig:
    """How much one rule can cost a file.

    A finding costs `base` points when it fires exactly at the threshold, then
    grows linearly — one further `base` per `scale` units past the threshold —
    until it hits `cap`. Growth matters: a metric an LLM optimises against needs
    to keep rewarding progress on a bad function, not go flat the moment it
    stops being the worst offender.
    """

    #: Points deducted for a finding sitting exactly at the threshold.
    base: float
    #: Units past the threshold that add one more `base` worth of penalty.
    scale: float = 0.0
    #: Ceiling for a single finding.
    cap: float = 2.0
    #: Ceiling for all findings of this rule in one file. Stops a file with 40
    #: mediocre functions from being indistinguishable from one with 400.
    file_cap: float = 4.0
    #: 0.0 disables the rule, 1.0 is full strength — same semantics as
    #: CodeScene's rule weights.
    weight: float = 1.0

    def penalty(self, value: float, threshold: float) -> float:
        if self.weight <= 0.0:
            return 0.0
        excess = max(0.0, value - threshold)
        growth = (excess / self.scale) if self.scale > 0 else 0.0
        return min(self.base * (1.0 + growth), self.cap) * self.weight


@dataclass(frozen=True)
class Profile:
    name: str
    thresholds: dict[str, float]
    rules: dict[str, RuleConfig]
    #: Glob patterns identifying test code, where the assertion rules apply and
    #: size limits are relaxed.
    test_patterns: tuple[str, ...] = ()
    #: Glob patterns excluded from scanning entirely.
    exclude_patterns: tuple[str, ...] = ()
    #: Rules that do not apply to test code. Test classes are *supposed* to hold
    #: independent methods that share no state, so design rules about cohesion
    #: fire constantly on a perfectly good test suite.
    test_exempt_rules: frozenset[str] = frozenset()

    def threshold(self, key: str) -> float:
        return self.thresholds[key]

    def rule(self, category: str) -> RuleConfig:
        return self.rules[category]

    def is_test_path(self, path: str) -> bool:
        posix = Path(path).as_posix()
        return any(fnmatch(posix, pattern) for pattern in self.test_patterns)

    def is_excluded(self, path: str) -> bool:
        posix = Path(path).as_posix()
        parts = Path(posix).parts
        for pattern in self.exclude_patterns:
            if fnmatch(posix, pattern) or any(
                fnmatch(part, pattern.strip("*/")) for part in parts
            ):
                return True
        return False


DEFAULT_THRESHOLDS: dict[str, float] = {
    # Function level
    "function_cyclomatic_complexity_warning": 10,
    "function_nesting_depth_warning": 3,
    "function_lines_of_code_warning": 60,
    "function_max_arguments": 4,
    "constructor_max_arguments": 5,
    "function_string_arguments_warning": 3,
    "function_primitive_arguments_min_count": 4,
    "function_primitive_arguments_ratio": 0.75,
    # Bumpy Road: a chunk counts as a bump at this nesting depth, and the rule
    # fires once a function contains this many bumps.
    "bumpy_road_min_bump_depth": 2,
    "bumpy_road_min_bumps": 2,
    # A conditional is "complex" at this many boolean operators; the rule fires
    # once a function holds this many such conditionals.
    "complex_conditional_min_operators": 2,
    "complex_conditional_min_count": 1,
    # Brain Method is compound: all three must hold at once.
    "brain_method_lines_of_code": 45,
    "brain_method_cyclomatic_complexity": 12,
    "brain_method_nesting_depth": 3,
    # Duplication
    "function_duplication_min_lines_of_code_for_check": 15,
    "function_duplication_min_similarity_percentage": 85,
    # Test code
    "assertion_block_max_statements": 8,
    "duplicated_assertion_block_min_statements": 3,
    # File / module level
    "file_lines_of_code_warning": 500,
    "file_declaration_lines_warning": 300,
    "module_max_functions": 20,
    "module_mean_complexity_warning": 6,
    "class_max_lines_of_code": 300,
    "class_max_functions": 15,
    "class_max_lcom4": 1,
    # Test files carry this multiplier on size limits; long test files are
    # normal and punishing them teaches the wrong lesson.
    "test_file_size_multiplier": 3.0,
}

DEFAULT_RULES: dict[str, RuleConfig] = {
    # Function level. Complex Method and Deep Nesting carry the most weight:
    # they are the two factors with the strongest published link to defect risk.
    COMPLEX_METHOD: RuleConfig(base=0.6, scale=10, cap=2.5, file_cap=5.0),
    DEEP_NESTED_COMPLEXITY: RuleConfig(base=0.8, scale=2, cap=2.5, file_cap=5.0),
    LARGE_METHOD: RuleConfig(base=0.5, scale=60, cap=2.0, file_cap=4.0),
    BUMPY_ROAD: RuleConfig(base=0.5, scale=2, cap=2.0, file_cap=4.0),
    COMPLEX_CONDITIONAL: RuleConfig(base=0.4, scale=2, cap=1.5, file_cap=3.0),
    EXCESS_ARGUMENTS: RuleConfig(base=0.4, scale=3, cap=1.5, file_cap=3.0),
    BRAIN_METHOD: RuleConfig(base=1.5, scale=15, cap=3.0, file_cap=4.5),
    CONSTRUCTOR_OVER_INJECTION: RuleConfig(base=0.5, scale=3, cap=1.5, file_cap=2.0),
    STRING_HEAVY_ARGUMENTS: RuleConfig(base=0.3, scale=2, cap=1.0, file_cap=2.0),
    PRIMITIVE_OBSESSION: RuleConfig(base=0.4, scale=0, cap=0.4, file_cap=1.6),
    CODE_DUPLICATION: RuleConfig(base=0.8, scale=0, cap=0.8, file_cap=3.0),
    # Test-specific
    LARGE_ASSERTION_BLOCKS: RuleConfig(base=0.3, scale=8, cap=1.0, file_cap=2.0),
    DUPLICATED_ASSERTION_BLOCKS: RuleConfig(base=0.4, scale=0, cap=0.4, file_cap=2.0),
    # File / module level
    FILE_SIZE: RuleConfig(base=0.5, scale=500, cap=2.0, file_cap=2.0),
    DECLARATION_SIZE: RuleConfig(base=0.3, scale=300, cap=1.0, file_cap=1.0),
    FUNCTION_COUNT: RuleConfig(base=0.4, scale=15, cap=1.5, file_cap=1.5),
    OVERALL_COMPLEXITY: RuleConfig(base=0.6, scale=4, cap=2.5, file_cap=2.5),
    BRAIN_CLASS: RuleConfig(base=1.5, scale=0, cap=1.5, file_cap=3.0),
    LOW_COHESION: RuleConfig(base=0.6, scale=2, cap=2.0, file_cap=2.0),
}

DEFAULT_TEST_PATTERNS: tuple[str, ...] = (
    "*/test_*.py",
    "test_*.py",
    "*_test.py",
    "*/tests/*",
    "tests/*",
    "*/conftest.py",
    "conftest.py",
    "*.test.ts",
    "*.test.tsx",
    "*.test.js",
    "*.test.jsx",
    "*.spec.ts",
    "*.spec.tsx",
    "*.spec.js",
    "*.spec.jsx",
    "*/__tests__/*",
    "*/e2e/*",
)

DEFAULT_EXCLUDE_PATTERNS: tuple[str, ...] = (
    "*/node_modules/*",
    "*/.venv/*",
    "*/venv/*",
    "*/.git/*",
    "*/__pycache__/*",
    "*/migrations/*",
    "*/.next/*",
    "*/dist/*",
    "*/build/*",
    "*/staticfiles/*",
    "*/site-packages/*",
    "*.min.js",
    "*.d.ts",
    "*/coverage/*",
    "*/.mypy_cache/*",
    "*/.pytest_cache/*",
    # Vendored shadcn/ui primitives. These are generated third-party components,
    # and their near-identical thin wrappers are that library's intended design,
    # not duplication anyone should "fix". Drop this pattern if you have started
    # treating them as your own code.
    "*/components/ui/*",
)

#: Validated against real test suites: every one of these fires on idiomatic,
#: well-written test code, so charging for them would only teach bad habits.
DEFAULT_TEST_EXEMPT_RULES = frozenset(
    {
        LOW_COHESION,  # independent test methods share no state, by design
        BRAIN_CLASS,  # a big test class is a big test class
        FUNCTION_COUNT,  # many small tests per module is the goal
        PRIMITIVE_OBSESSION,  # fixtures are built from literals
        STRING_HEAVY_ARGUMENTS,
    }
)

DEFAULT_PROFILE = Profile(
    name="artifer",
    thresholds=dict(DEFAULT_THRESHOLDS),
    rules=dict(DEFAULT_RULES),
    test_patterns=DEFAULT_TEST_PATTERNS,
    exclude_patterns=DEFAULT_EXCLUDE_PATTERNS,
    test_exempt_rules=DEFAULT_TEST_EXEMPT_RULES,
)

#: Filenames searched, nearest-first, when no profile is passed explicitly.
CONFIG_FILENAMES = (".aqs.json", ".aqs/config.json")


class ProfileError(ValueError):
    """Raised when a config file is present but unusable."""


def _apply_overrides(profile: Profile, data: dict[str, Any]) -> Profile:
    thresholds = dict(profile.thresholds)
    for key, value in (data.get("thresholds") or {}).items():
        if key not in DEFAULT_THRESHOLDS:
            raise ProfileError(
                f"unknown threshold {key!r}. Run `aqs explain --thresholds` for valid names."
            )
        if not isinstance(value, (int, float)):
            raise ProfileError(f"threshold {key!r} must be a number, got {value!r}")
        thresholds[key] = float(value)

    rules = dict(profile.rules)
    for name, override in (data.get("rules") or {}).items():
        if name not in DEFAULT_RULES:
            raise ProfileError(
                f"unknown rule {name!r}. Run `aqs explain --list` for valid names."
            )
        if isinstance(override, (int, float)):
            override = {"weight": float(override)}
        if not isinstance(override, dict):
            raise ProfileError(f"rule {name!r} must be a number or an object")
        unknown = set(override) - {"base", "scale", "cap", "file_cap", "weight"}
        if unknown:
            raise ProfileError(
                f"rule {name!r} has unknown field(s): {', '.join(sorted(unknown))}"
            )
        rules[name] = replace(rules[name], **{k: float(v) for k, v in override.items()})

    return Profile(
        name=str(data.get("name", profile.name)),
        thresholds=thresholds,
        rules=rules,
        test_patterns=tuple(data.get("test_patterns", profile.test_patterns)),
        exclude_patterns=tuple(data.get("exclude_patterns", profile.exclude_patterns)),
        test_exempt_rules=frozenset(
            data.get("test_exempt_rules", profile.test_exempt_rules)
        ),
    )


def find_config(start: Path) -> Path | None:
    """Nearest config file at or above `start`."""
    start = start if start.is_dir() else start.parent
    for directory in (start.resolve(), *start.resolve().parents):
        for filename in CONFIG_FILENAMES:
            candidate = directory / filename
            if candidate.is_file():
                return candidate
    return None


def load_profile(root: Path, explicit: Path | None = None) -> Profile:
    """The default profile with any config file's overrides applied."""
    config = explicit or find_config(root)
    if config is None:
        return DEFAULT_PROFILE
    try:
        data = json.loads(config.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise ProfileError(f"{config}: invalid JSON ({exc})") from exc
    if not isinstance(data, dict):
        raise ProfileError(f"{config}: expected a JSON object at the top level")
    return _apply_overrides(DEFAULT_PROFILE, data)
