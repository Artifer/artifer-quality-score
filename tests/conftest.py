from __future__ import annotations

import pytest

from aqs.analyze import analyze_source
from aqs.languages import JAVASCRIPT, PYTHON, TYPESCRIPT
from aqs.models import FileReport
from aqs.profile import DEFAULT_PROFILE, Profile

SPECS = {"python": PYTHON, "typescript": TYPESCRIPT, "javascript": JAVASCRIPT}


@pytest.fixture
def profile() -> Profile:
    return DEFAULT_PROFILE


@pytest.fixture
def score():
    """Analyse a source string. `path` decides test-vs-production treatment."""

    def _score(
        source: str,
        path: str = "sample.py",
        language: str = "python",
        custom_profile: Profile | None = None,
    ) -> FileReport:
        return analyze_source(
            path,
            source.encode("utf-8"),
            SPECS[language],
            custom_profile or DEFAULT_PROFILE,
        )

    return _score


@pytest.fixture
def categories():
    """The set of rule names a report contains."""

    def _categories(report: FileReport) -> set[str]:
        return {finding.category for finding in report.all_findings}

    return _categories


@pytest.fixture
def find():
    """The first finding for a given rule, or None."""

    def _find(report: FileReport, category: str):
        for finding in report.all_findings:
            if finding.category == category:
                return finding
        return None

    return _find
