"""Command line interface.

Five commands, matching the moments you actually want a quality number:

    aqs scan      what is the state of this code
    aqs delta     did this change make it worse   (the TDD / pre-commit loop)
    aqs gate      may this merge into develop     (whole-repo, no-regression rule)
    aqs check     lint-style lines for an editor or a grep
    aqs explain   what does this rule mean and how do I fix it

`delta` and `gate` are not the same check. `delta` compares only changed files and
is fast enough to run on save. `gate` scores both trees in full, which is the only
way to catch a repository-score drop caused by LoC weight shift.

Exit codes are the contract for automation: 0 means the gate passed, 1 means it
failed, 2 means the tool could not run.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Annotated, Optional

import typer
from rich.console import Console

from . import render
from .analyze import scan as run_scan
from .cache import open_cache
from .delta import compute_delta
from .explain import RULE_DOCS, lookup
from .gate import compute_gate
from .gitio import GitError
from .models import ScanReport
from .profile import Profile, ProfileError, load_profile
from .version import __version__

app = typer.Typer(
    name="aqs",
    help=(
        "Deterministic, explainable code quality scoring for Python, TypeScript "
        "and JavaScript."
    ),
    no_args_is_help=True,
    add_completion=False,
)

EXIT_OK = 0
EXIT_GATE_FAILED = 1
EXIT_ERROR = 2

console = Console()
error_console = Console(stderr=True)


def _load(root: Path, explicit: Optional[Path]) -> Profile:
    try:
        return load_profile(root, explicit)
    except ProfileError as exc:
        error_console.print(f"[red]config error:[/red] {exc}")
        raise typer.Exit(EXIT_ERROR)


def _emit_json(payload: object) -> None:
    """Write machine-readable output on plain stdout.

    Deliberately not `rich.print_json`: that parses and syntax-highlights the whole
    document, which costs seconds on a large report and is pure waste for output
    that is being piped into another program.
    """
    text = payload if isinstance(payload, str) else json.dumps(payload, indent=2)
    print(text)


def _guidance_for(report: ScanReport) -> dict[str, dict[str, str]]:
    """Fix guidance for just the rules that actually fired."""
    present = {
        finding.category
        for file_report in report.files
        for finding in file_report.all_findings
    }
    return {
        category: {
            "summary": doc.summary,
            "why": doc.why,
            "fix": doc.fix,
        }
        for category, doc in RULE_DOCS.items()
        if category in present
    }


def _version_callback(value: bool) -> None:
    if value:
        console.print(f"aqs {__version__}")
        raise typer.Exit(EXIT_OK)


@app.callback()
def root(
    version: Annotated[
        bool,
        typer.Option(
            "--version", callback=_version_callback, is_eager=True, help="Show version."
        ),
    ] = False,
) -> None:
    pass


@app.command()
def scan(
    paths: Annotated[
        Optional[list[Path]],
        typer.Argument(help="Files or directories to scan. Defaults to the CWD."),
    ] = None,
    as_json: Annotated[
        bool, typer.Option("--json", help="Emit the full report as JSON.")
    ] = False,
    guidance: Annotated[
        bool,
        typer.Option(
            "--guidance",
            help=(
                "With --json, embed fix guidance for every rule that fired. Use this "
                "when handing the report to a model."
            ),
        ),
    ] = False,
    show_all: Annotated[
        bool, typer.Option("--all", help="Show every file with findings.")
    ] = False,
    limit: Annotated[
        int, typer.Option("--limit", "-n", help="How many files to show.")
    ] = 10,
    min_score: Annotated[
        Optional[float],
        typer.Option(
            "--min-score",
            help="Exit non-zero if the aggregate score falls below this.",
        ),
    ] = None,
    profile_path: Annotated[
        Optional[Path], typer.Option("--profile", help="Path to a config JSON file.")
    ] = None,
    jobs: Annotated[
        Optional[int],
        typer.Option(
            "--jobs",
            "-j",
            help=(
                "Worker processes for large trees. Defaults to the CPU count. "
                "The score is identical whatever you set."
            ),
        ),
    ] = None,
    no_cache: Annotated[
        bool,
        typer.Option("--no-cache", help="Disable the .aqs/ report cache."),
    ] = False,
) -> None:
    """Score a tree and report the lowest-scoring files."""
    targets = [p for p in (paths or [Path.cwd()])]
    for target in targets:
        if not target.exists():
            error_console.print(f"[red]error:[/red] no such path: {target}")
            raise typer.Exit(EXIT_ERROR)

    base = targets[0] if targets[0].is_dir() else targets[0].parent
    profile = _load(base, profile_path)
    with open_cache(base, profile, enabled=not no_cache) as cache:
        report = run_scan(targets, profile, relative_to=base, jobs=jobs, cache=cache)

    if as_json:
        payload = report.model_dump(mode="json")
        if guidance:
            payload["guidance"] = _guidance_for(report)
        _emit_json(payload)
    else:
        render.render_scan(report, console, limit=limit, show_clean=show_all)

    if min_score is not None and report.score < min_score:
        if not as_json:
            error_console.print(
                f"[red]score {report.score:.2f} is below the required "
                f"{min_score:.2f}[/red]"
            )
        raise typer.Exit(EXIT_GATE_FAILED)


@app.command()
def delta(
    baseline: Annotated[
        str, typer.Option("--baseline", "-b", help="Git ref to compare against.")
    ] = "HEAD",
    staged: Annotated[
        bool,
        typer.Option("--staged", help="Score the index instead of the working tree."),
    ] = False,
    max_drop: Annotated[
        float,
        typer.Option(
            "--max-drop",
            help=(
                "Allowed aggregate score decrease before failing. 0 fails on any "
                "regression."
            ),
        ),
    ] = 0.0,
    as_json: Annotated[
        bool, typer.Option("--json", help="Emit the delta report as JSON.")
    ] = False,
    path: Annotated[
        Optional[Path], typer.Option("--path", help="Repository to inspect.")
    ] = None,
    profile_path: Annotated[
        Optional[Path], typer.Option("--profile", help="Path to a config JSON file.")
    ] = None,
    merge_base: Annotated[
        bool,
        typer.Option(
            "--merge-base",
            help=(
                "Compare against the merge base with --baseline rather than its "
                "tip, so only your own changes count."
            ),
        ),
    ] = False,
    jobs: Annotated[
        Optional[int], typer.Option("--jobs", "-j", help="Worker processes.")
    ] = None,
) -> None:
    """Compare changed files against a git baseline. Fails on regression.

    Fast enough for a pre-commit hook. For the "repository score never goes down"
    rule, use `aqs gate` — this command's aggregate covers only changed files.
    """
    root_path = path or Path.cwd()
    profile = _load(root_path, profile_path)
    try:
        report = compute_delta(
            root_path,
            profile,
            baseline=baseline,
            staged=staged,
            merge_base=merge_base,
            jobs=jobs,
        )
    except GitError as exc:
        error_console.print(f"[red]git error:[/red] {exc}")
        raise typer.Exit(EXIT_ERROR)

    if as_json:
        _emit_json(json.loads(report.model_dump_json()))
    else:
        render.render_delta(report, console, threshold=max_drop)

    if report.score_change < -abs(max_drop):
        raise typer.Exit(EXIT_GATE_FAILED)


@app.command()
def gate(
    against: Annotated[
        str,
        typer.Option(
            "--against",
            "-a",
            help="Branch or ref to compare the whole repository against.",
        ),
    ] = "origin/develop",
    tolerance: Annotated[
        float,
        typer.Option(
            "--tolerance",
            help="Allowed repository score drop. 0 means it may never go down.",
        ),
    ] = 0.0,
    no_merge_base: Annotated[
        bool,
        typer.Option(
            "--no-merge-base",
            help=(
                "Compare against the ref's tip instead of the merge base. Off by "
                "default, so the gate judges only your own changes."
            ),
        ),
    ] = False,
    as_json: Annotated[
        bool, typer.Option("--json", help="Emit the gate report as JSON.")
    ] = False,
    path: Annotated[
        Optional[Path], typer.Option("--path", help="Repository to inspect.")
    ] = None,
    profile_path: Annotated[
        Optional[Path], typer.Option("--profile", help="Path to a config JSON file.")
    ] = None,
    jobs: Annotated[
        Optional[int], typer.Option("--jobs", "-j", help="Worker processes.")
    ] = None,
    no_cache: Annotated[
        bool,
        typer.Option(
            "--no-cache",
            help=(
                "Disable the .aqs/ report cache. The cache never changes a score, "
                "only how long it takes to get one."
            ),
        ),
    ] = False,
) -> None:
    """Enforce that the whole repository's score never goes down.

    Scores both trees in full, unlike `delta`, so it also catches a drop caused
    purely by weight shift — a new large file that is individually fine but below
    the repository's average.
    """
    root_path = path or Path.cwd()
    profile = _load(root_path, profile_path)
    try:
        report = compute_gate(
            root_path,
            profile,
            against=against,
            tolerance=tolerance,
            merge_base=not no_merge_base,
            jobs=jobs,
            use_cache=not no_cache,
        )
    except GitError as exc:
        error_console.print(f"[red]git error:[/red] {exc}")
        raise typer.Exit(EXIT_ERROR)

    if as_json:
        _emit_json(json.loads(report.model_dump_json()))
    else:
        render.render_gate(report, console)

    if not report.passed:
        raise typer.Exit(EXIT_GATE_FAILED)


@app.command()
def check(
    paths: Annotated[
        Optional[list[Path]],
        typer.Argument(help="Files or directories to check. Defaults to the CWD."),
    ] = None,
    fail_on_findings: Annotated[
        bool,
        typer.Option("--fail-on-findings", help="Exit non-zero if anything is found."),
    ] = False,
    profile_path: Annotated[
        Optional[Path], typer.Option("--profile", help="Path to a config JSON file.")
    ] = None,
    jobs: Annotated[
        Optional[int], typer.Option("--jobs", "-j", help="Worker processes.")
    ] = None,
) -> None:
    """Emit findings as lint-style `path:line:col: rule: details` lines."""
    targets = [p for p in (paths or [Path.cwd()])]
    for target in targets:
        if not target.exists():
            error_console.print(f"[red]error:[/red] no such path: {target}")
            raise typer.Exit(EXIT_ERROR)

    base = targets[0] if targets[0].is_dir() else targets[0].parent
    profile = _load(base, profile_path)
    report = run_scan(targets, profile, relative_to=base, jobs=jobs)
    render.render_check(report, console)

    if fail_on_findings and report.findings_count:
        raise typer.Exit(EXIT_GATE_FAILED)


@app.command()
def explain(
    rule: Annotated[
        Optional[str], typer.Argument(help="Rule name, e.g. 'Bumpy Road Ahead'.")
    ] = None,
    list_rules: Annotated[
        bool, typer.Option("--list", help="List every rule and its weight.")
    ] = False,
    thresholds: Annotated[
        bool, typer.Option("--thresholds", help="List every configurable threshold.")
    ] = False,
    as_json: Annotated[
        bool, typer.Option("--json", help="Emit the guidance as JSON.")
    ] = False,
    profile_path: Annotated[
        Optional[Path], typer.Option("--profile", help="Path to a config JSON file.")
    ] = None,
) -> None:
    """Explain what a rule means, why it matters, and how to fix it."""
    profile = _load(Path.cwd(), profile_path)

    if thresholds:
        if as_json:
            _emit_json(profile.thresholds)
        else:
            render.render_thresholds(profile, console)
        return

    if list_rules or rule is None:
        if as_json:
            _emit_json(
                {
                    category: {
                        "summary": doc.summary,
                        "why": doc.why,
                        "fix": doc.fix,
                        "weight": profile.rules[category].weight,
                        "thresholds": {
                            key: profile.thresholds[key] for key in doc.thresholds
                        },
                    }
                    for category, doc in RULE_DOCS.items()
                }
            )
        else:
            render.render_rule_list(profile, console)
        return

    doc = lookup(rule)
    if doc is None:
        error_console.print(
            f"[red]error:[/red] unknown rule {rule!r}. Run `aqs explain --list`."
        )
        raise typer.Exit(EXIT_ERROR)

    if as_json:
        _emit_json(
            {
                "category": doc.category,
                "summary": doc.summary,
                "why": doc.why,
                "fix": doc.fix,
                "thresholds": {key: profile.thresholds[key] for key in doc.thresholds},
            }
        )
    else:
        render.render_explain(doc, profile, console)


def main() -> None:
    app()


if __name__ == "__main__":  # pragma: no cover
    main()
