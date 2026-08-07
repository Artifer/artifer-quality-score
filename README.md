# Artifer Quality Score

A deterministic, explainable code quality score for Python, TypeScript and
JavaScript. One number per file from 1.0 to 10.0, with every deducted point
traceable to a named code smell at a specific line.

**Two implementations, one behaviour.** `rust/` is the primary implementation —
a single static binary, no runtime to install. `src/aqs/` is the original Python
version, kept as a differential-testing oracle. They are verified to produce
byte-identical JSON across 1,191 files and 3,841 findings; see
[Implementations](#implementations).

Built for two jobs:

1. **A fast quality read inside a TDD loop.** `aqs delta` scores only what
   changed and fails on regression, so it can run on every commit without
   becoming noise.
2. **A gradient for generated code to climb.** `aqs scan --json --guidance`
   emits findings *and* the remediation guidance for every rule that fired, so a
   model gets told what is wrong and how to fix it — not just a number.

```
  Artifer Quality Score   profile artifer · v0.1.0

  9.13 / 10   79 files · 4,196 LoC · 26 findings

  Lowest scoring files (2 of 10)

   3.30  apps/pipeline/tasks.py  (184 LoC)
     68  Deep, Nested Complexity     run_full_pipeline_task  Nesting depth = 6 …  -2.00
      1  Overall Code Complexity                             mean cc = 9.8 acro…  -1.16
    159  Complex Method              rewrite_chapter_task    cc = 19              -1.14
```

## Install

```bash
cd rust && cargo build --release      # binary at rust/target/release/aqs
```

The Python reference implementation, if you want it for differential testing:

```bash
uv venv && uv pip install -e .
```

## Usage

```bash
aqs scan .                      # full report, worst files first
aqs scan . --json --guidance    # machine-readable, with fix guidance
aqs scan . --min-score 7.0      # exit 1 if the repo falls below 7.0

aqs delta                       # changed files vs git HEAD; exit 1 on regression
aqs delta --staged              # score the index — use this in a pre-commit hook
aqs delta --max-drop 0.5        # tolerate a small regression

aqs gate --against origin/develop   # whole repo: the score may never go down
aqs gate --tolerance 0.25           # allow a small drop
aqs gate --json                     # machine-readable, for a CI summary

aqs check src/                  # lint-style path:line:col: rule: details
aqs check src/ --fail-on-findings

aqs explain "Bumpy Road Ahead"  # what it means, why it matters, how to fix it
aqs explain --list              # every rule and its weight
aqs explain --thresholds        # every configurable threshold
```

Exit codes: `0` gate passed, `1` gate failed, `2` the tool could not run.

### The merge gate: "the score may never go down"

`delta` and `gate` are **not the same check**, and using the wrong one leaves a
hole:

| | `aqs delta` | `aqs gate` |
| --- | --- | --- |
| Scores | changed files only | the whole tree, both sides |
| Answers | "did I make this worse?" | "did the repository score drop?" |
| Cost | 40-150ms | ~1.7s cold, ~0.2s cached (100k LoC) |
| Use in | pre-commit, watch loop | CI, merge gate |

The gap matters. Add an 800-line file scoring 8.5 to a repo averaging 9.3 and
nothing has regressed — every file is fine, no smell was introduced — but the
repository's LoC-weighted score still falls. `delta` reports nothing; `gate`
catches it. Enforcing "never goes down" therefore requires scoring both trees in
full, which is what `gate` does.

The baseline is the **merge base** with the target branch by default, so the gate
judges only your own changes. Comparing against the tip of `develop` would fail
your branch for regressions someone else merged while you were working. Use
`--no-merge-base` if you genuinely want tip-to-tip.

Ready-to-use integrations are in [`examples/`](examples/):

- [`github-workflow-quality-gate.yml`](examples/github-workflow-quality-gate.yml) —
  a PR gate on `[main, staging, develop]` that fails on any drop and writes a
  score table to the job summary. Needs `fetch-depth: 0` (no history, no merge
  base) and the `actions/cache` step (without it every PR re-scores all of
  develop).
- [`pre-commit-hook.yaml`](examples/pre-commit-hook.yaml) — the fast local loop.

### In a TDD loop

```bash
aqs delta --staged || echo "quality regressed — check before committing"
```

### Handing a report to a model

```bash
aqs scan src/thing.py --json --guidance
```

The payload contains each finding's `category`, `details`, `penalty`,
`value`/`threshold`, and a `guidance` map explaining every rule that fired. That
is enough context to act without any further prompting.

## How the score works

```
score = 10 - Σ penalties,  floored at 1.0
```

That is the whole model, deliberately. A finding costs its rule's `base` when it
fires exactly at the threshold, then grows linearly — one further `base` per
`scale` units past it — up to a `cap`.

Growth matters for the second job: a metric a model optimises against has to keep
rewarding progress on a bad function, not go flat as soon as it stops being the
worst offender.

Two guards keep the arithmetic honest:

- **Per-rule file caps** stop one rule consuming the whole budget. A file with
  forty mildly complex functions should not look identical to one with four
  hundred, but nor should it drown out everything else wrong with the file.
- **Largest-first cap allocation** means that when a cap binds, the worst
  offenders keep their full penalty and the marginal ones get trimmed. Fixing the
  worst function therefore always moves the score. Findings that scored zero
  because their rule's budget was spent are shown as `capped` rather than hidden.

Repo aggregation is a **LoC-weighted mean**, so a 5000-line mess is not offset by
ten clean stubs. (This is the one part of CodeScene's method that is fully
published — because the naive alternative is actively misleading.)

## The rules

Function level:

| Rule | Fires when |
| --- | --- |
| Complex Method | cyclomatic complexity > 10 |
| Large Method | > 60 lines of code |
| Deep, Nested Complexity | nesting depth > 3 |
| Bumpy Road Ahead | ≥ 2 separate chunks of nested logic |
| Complex Conditional | a condition combines ≥ 2 boolean operators |
| Excess Number of Function Arguments | > 4 arguments |
| Constructor Over-Injection | a constructor takes > 5 arguments |
| Brain Method | long **and** complex **and** deeply nested, all at once |
| String Heavy Function Arguments | > 3 string-typed parameters |
| Primitive Obsession | ≥ 4 annotated params, ≥ 75% of them primitives |

File and module level:

| Rule | Fires when |
| --- | --- |
| Lines of Code in a Single File | > 500 lines |
| Lines of Declarations in a Single File | > 300 lines of types/interfaces (TS) |
| Number of Functions in a Single Module | > 20 |
| Overall Code Complexity | mean cc > 6 across ≥ 3 functions |
| Brain Class | large, many methods, and contains a Brain Method |
| Low Cohesion | LCOM4 > 1 — methods form unrelated groups |
| Code Duplication | two functions ≥ 15 LoC that are ≥ 85% structurally similar |

Test files only:

| Rule | Fires when |
| --- | --- |
| Large Assertion Blocks | > 8 consecutive assertions |
| Duplicated Assertion Blocks | the same assertion block appears ≥ 2 times |

### Design decisions worth knowing

- **Named nested functions are measured separately; anonymous ones are not.** A
  named helper (`def helper()`, `const load = () => {}`) gets its own score. An
  inline callback (`items.map(x => ...)`) counts toward its parent, because that
  is where its complexity lands when you read the code.
- **Comments and docstrings never count as lines of code.** Otherwise the
  cheapest way to raise a score is to delete the documentation.
- **`elif` / `else if` chains are flat, not nested.** They read as sibling
  branches; counting them as a staircase would flag every dispatch function.
- **Only annotated parameters count for type smells.** An unannotated parameter
  is absence of evidence, not evidence of bad design — otherwise the score would
  measure how thoroughly the code is typed rather than how well it is designed.
- **Interfaces are exempt from Low Cohesion.** ABCs, Protocols and all-stub
  classes have no shared state by definition. Flagging them would be telling you
  to stop declaring interfaces.
- **Test classes are exempt from cohesion and size rules.** Independent test
  methods are the goal. See `test_exempt_rules`.
- **Vendored code is excluded by default**, including `components/ui/*` — those
  are generated shadcn/ui primitives whose near-identical thin wrappers are that
  library's intended design, not duplication to fix.

## Configuration

Drop a `.aqs.json` at your repo root. It is discovered upwards from the scan
target. Unknown rule or threshold names are a hard error, never silently ignored.

```json
{
  "name": "my-project",
  "thresholds": {
    "function_cyclomatic_complexity_warning": 12,
    "file_lines_of_code_warning": 400
  },
  "rules": {
    "Primitive Obsession": 0.0,
    "Complex Method": { "base": 0.8, "cap": 3.0 }
  },
  "exclude_patterns": ["*/legacy/*"]
}
```

A rule's value can be a bare number (its weight, `0.0` disables it) or an object
overriding `base`, `scale`, `cap`, `file_cap` or `weight`.

Run `aqs explain --thresholds` for the full list of threshold names.

## Performance

Measured on a 258,955 LoC mixed Python/TS corpus, 12-core M-series, all cores,
end-to-end through the CLI including JSON output.

**Bulk throughput — 3.4x:**

| | Time | Throughput | 100k LoC |
| --- | --- | --- | --- |
| Python | 2.22s | 116,747 LoC/s | 0.86s |
| **Rust** | **0.66s** | **392,102 LoC/s** | **0.26s** |

**Small-scale latency — 12-14x.** This is the path an agent loop actually hits, and
the gap is widest here because Python's interpreter startup dominates it entirely
(best of 8):

| Case | Python | Rust | Speedup |
| --- | --- | --- | --- |
| single file (`check`) | 162ms | 14ms | 11.8x |
| 16-file directory (`scan`) | 239ms | 17ms | 14.2x |
| startup only (`explain`) | 122ms | 2ms | 74.4x |

Python's ~122ms of interpreter and import cost is unavoidable and paid on every
invocation; Rust pays 2ms. For a loop that scores a file after each edit, that fixed
cost *is* the runtime.

> An earlier version of this table claimed 7.0x on bulk throughput. That was wrong:
> about half the Python side was `rich.print_json` syntax-highlighting 2.6 MB of
> piped output, not analysis. The Python CLI now writes JSON to plain stdout and the
> honest figure is 3.4x. Decompose a benchmark before believing it.

Full two-sided merge gate, Python, on the same corpus — it scores every file in two
trees, so this is the expensive case:

| | Time | 100k LoC equivalent |
| --- | --- | --- |
| `--no-cache` | 4.30s | 1.69s |
| cold cache | 2.42s | 0.95s |
| **warm cache** | **0.57s** | **0.22s** |

All produce an identical score. Caching and the Rust port are independent levers
that compose: caching removes work, Rust does the remaining work faster.

Both implementations parallelise (rayon in Rust, a process pool in Python) and
collect results in input order, so the report is byte-identical however many
workers ran. The Python pool falls back to serial rather than failing if it cannot
spawn — parallelism is an optimisation, never a correctness requirement.

`aqs delta` only scores changed files, so the TDD-loop path is typically 40-150ms
regardless of repo size. Baseline blobs are read through a single
`git cat-file --batch` process — a `git show` per file means two subprocesses per
file, which on a large branch diff costs more than all the parsing.

### The cache

A file's report is a pure function of (file bytes, profile, tool version), so it
is trivially cacheable. `.aqs/cache.sqlite3` stores reports keyed by content hash;
add `.aqs/` to `.gitignore`.

This is where the real speed is, and it is worth understanding why it beats making
the analysis itself faster. The baseline side of a merge gate is an **immutable
commit** — after the first run, every one of its files is a cache hit. The current
side only re-analyses what actually changed. Caching removes the work rather than
doing it faster, which is a bigger lever than any constant-factor speedup:
measured warm-cache gate throughput is already faster than an optimistic 5x
rewrite of the uncached path would be.

The tool version and a fingerprint of the whole profile are part of every cache
key, so a score computed under different rules can never be served — changing a
threshold invalidates everything, by construction. Cache failures are never fatal:
an unwritable or corrupt cache degrades to no caching, because being unable to
write a cache file is not a reason to refuse to score someone's code.

Use `--no-cache` on `scan` and `gate` to bypass it.

## Determinism

A scan is a pure function of (file bytes, profile, tool version). No git history,
no network, no clock, no filesystem-order dependence. Same input, byte-identical
JSON out.

The `tree-sitter-*` grammar pins in `pyproject.toml` are **exact, not ranges**.
A grammar bump can rename AST nodes and silently change every score in the repo,
so treat changing a pin — or the tool version — as a scoring change that needs
re-baselining.

## What we left out, and why

- **All organisational and git-history factors.** CodeScene's Code Health also
  weighs Developer Congestion and Complex Code by Former Contributors. Those need
  repo history, cannot be computed for a single file, and say nothing about code a
  model just generated. Excluding them is what makes this a pure function of
  source bytes.
- **Brain Method's centrality score.** CodeScene combines size, complexity,
  nesting *and* a call-graph centrality measure. Centrality needs whole-repo
  indexing, which would cost the sub-second scan the TDD loop depends on. Size,
  complexity and depth together still isolate the right function.
- **Cross-file duplication.** Clone detection across a repo needs an index. Ours
  is intra-file only, for the same reason.

## Known limitations

Be aware of these before treating the score as a target:

- **Fragmentation is invisible.** Splitting a bad 300-line function across five
  files will raise the score without improving the design. `Number of Functions
  in a Single Module` catches within-module churning, but nothing catches
  spreading logic thinly across new files. If you point a model at this metric,
  review its diffs — do not just watch the number go up.
- **Naming, correctness and API design are not measured at all.** A perfectly
  scored file can still be wrong and unreadable. This measures structure.
- **Declarative framework signatures trip the argument rules.** A Typer/Click
  command, a FastAPI route handler or a DRF view with many query parameters
  legitimately has a wide signature — the parameters *are* the interface
  declaration. Scanning this tool's own `cli.py` flags `Excess Number of Function
  Arguments` on every command for exactly this reason. If that noise bothers you,
  lower the rule's weight for those paths rather than reshaping the code around
  the metric:

  ```json
  { "rules": { "Excess Number of Function Arguments": 0.3 } }
  ```

- **Test-file heuristics are path-based.** A test that does not match
  `test_patterns` gets production rules applied to it.
- **Primitive Obsession and String Heavy Arguments are weaker in untyped code**,
  by design — see above.
- **The weights are hand-tuned, not empirically calibrated.** They were adjusted
  against the Artifer codebases so that clean code lands at 10.0 and genuinely
  problematic files land in the 3-5 range. They are a considered starting point,
  not a validated instrument.

## Relationship to CodeScene

The rule *taxonomy* follows CodeScene's published code-health factors, which are
well documented and backed by peer-reviewed research (Tornhill & Borg, *Code Red:
The Business Impact of Code Quality*, TechDebt 2022). The `details` string
formats match theirs too, because they are compact and read well in a diff.

The *weights are entirely ours.* CodeScene's are calibrated against proprietary
baseline data and have been recalibrated repeatedly across their releases, so
there is no public number to copy and no claim of equivalence here.

**"CodeHealth" is a CodeScene trademark.** This is a separate, independent tool.
Its scores are not comparable to CodeScene's and should never be presented as
such.

## Implementations

`rust/` is primary. `src/aqs/` is the Python original, retained deliberately as a
**differential-testing oracle**: when a port has a known-good reference, every
disagreement is a bug with an answer already attached, which is what turns a rewrite
from a gamble into routine work.

```bash
python scripts/differential.py src ../backend ../frontend
#   IDENTICAL — 1191 files, 3841 findings, score 5.7138
```

Verified identical across 1,191 files / 3,841 findings, including every score,
penalty, `details` string and threshold. Parity is not automatic — three things had
to be ported exactly rather than reasonably:

- **`difflib.SequenceMatcher`**, including its autojunk heuristic. For sequences of
  200+ elements it drops any element occurring in more than 1% of the input, which
  changes real similarity ratios. Any "sensible" substitute (Levenshtein, Jaccard,
  a different LCS) gives different numbers and moves the duplication threshold.
- **`fnmatch` semantics**, where `*` matches `/`. A path-aware glob crate would
  silently break every exclude pattern in the profile.
- **Detector emission order**, because per-rule cap allocation breaks ties by
  discovery order, so a different traversal changes which findings get trimmed.

Two bugs surfaced during the port and were fixed in both: an unanchored regex in the
Rust glob translation, and a Python `lstrip("await ")` that stripped a *character
set* rather than a prefix, mangling `assertEqual(...)` into `ssertEqual(...)`.

Keeping both costs little — the Python version is frozen — and buys a permanent
conformance check for any future rule change.

## Development

```bash
cd rust && cargo test           # 69 tests
.venv/bin/python -m pytest      # 140 tests
python scripts/differential.py src tests
```

The architecture keeps detection and scoring strictly separate. Both
implementations use the same module layout, so a change lands in the same place
twice:

| Module | Responsibility |
| --- | --- |
| `languages` | per-language AST vocabulary — add a language here, no detector changes |
| `metrics` | AST measurement primitives (complexity, nesting, bumps, LoC) |
| `detectors` | the rule catalogue; reports measurements, knows nothing about points |
| `profile` | thresholds and weights — the entire scoring policy |
| `scoring` | findings → penalties → score |
| `analyze` | discovery, the analysis pipeline, parallel mapping |
| `cache` | content-addressed report cache |
| `gitio` | git primitives; scoring a revision without checking it out |
| `delta` | changed-file comparison |
| `gate` | whole-repository no-regression gate |
| `explain` | remediation guidance per rule |
| `difflib` | (Rust only) the ported similarity algorithm |

Adding a rule means writing one detector function with a `@function_detector` or
`@file_detector` decorator, one `RuleConfig` entry, and one `RuleDoc`. Because no
detector knows about points, the penalty model stays swappable — a future
`--profile=codescene` fitted against real `cs review` output would not require
touching a single detector.
