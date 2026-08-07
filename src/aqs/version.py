"""Single source of truth for the tool version.

Every report carries it. A score is only reproducible if you know which detector
and profile version produced it, so treat a bump here as a scoring change and
re-baseline accordingly.
"""

__version__ = "0.1.0"
