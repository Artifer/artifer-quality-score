"""Artifer Quality Score — deterministic, explainable code quality scoring.

A 1.0-10.0 score per file, derived from a catalogue of structural code smells,
built for two jobs: a fast quality read during a TDD loop, and a gradient an LLM
can actually climb when it is asked to improve code it just wrote.

The rule taxonomy follows CodeScene's published code-health factors. The penalty
weights are our own — theirs are proprietary. "CodeHealth" is CodeScene's
trademark; this is a separate tool and its numbers are not comparable to theirs.
"""

from .analyze import analyze_source, scan
from .models import DeltaReport, FileReport, Finding, ScanReport
from .profile import DEFAULT_PROFILE, Profile, load_profile
from .version import __version__

__all__ = [
    "DEFAULT_PROFILE",
    "DeltaReport",
    "FileReport",
    "Finding",
    "Profile",
    "ScanReport",
    "__version__",
    "analyze_source",
    "load_profile",
    "scan",
]
