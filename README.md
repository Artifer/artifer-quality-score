# Artifer Quality Score

A deterministic, explainable code quality score for Python, TypeScript, JavaScript,
Rust, Go, Ruby, Java and PHP, plus the `<script>` block of Vue and Svelte single-file
components. One number per
file from 1.0 to 10.0, with every deducted point traceable to a named code smell at
a specific line.

A single static binary — no runtime to install in the repository being scored,
which matters for Node-only frontend pipelines.

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

### From a release (no Rust toolchain needed)

```bash
curl -fsSL https://raw.githubusercontent.com/Artifer/artifer-quality-score/master/install.sh | bash
```

No token, no `gh`, no Rust — just `curl` and `tar`. The download is checksum-verified
against the `SHA256SUMS` published with the release, which is worth insisting on for
anything piped into a shell.

Read it before running it, if you prefer:

```bash
git clone https://github.com/Artifer/artifer-quality-score.git
cd artifer-quality-score && ./install.sh
```

Installs to `~/.local/bin`; set `PREFIX=/usr/local/bin` to change that, and pass a tag
to pin a version:

```bash
PREFIX=/usr/local/bin ./install.sh v0.3.0
```

macOS binaries are universal, so one download covers Apple Silicon and Intel. Linux is
built for both x86_64 and arm64.

Manual download, if you would rather not pipe a script into a shell. Every asset name
embeds its version, so `releases/latest/download/` cannot be used without naming the
version anyway — set it once:

```bash
VERSION=v0.7.1   # or whichever release you want
curl -fsSLO "https://github.com/Artifer/artifer-quality-score/releases/download/$VERSION/aqs-$VERSION-macos-universal.tar.gz"
tar -xzf aqs-*.tar.gz && install -m 755 aqs ~/.local/bin/aqs
```

Swap `macos-universal` for `x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` on
Linux. `SHA256SUMS` is published alongside; `install.sh` verifies it for you.

### From source

```bash
cargo build --release          # binary at target/release/aqs
cargo install --path .         # or put it on your PATH
```

### Cutting a new release

Bump `version` in `Cargo.toml` **and refresh `Cargo.lock`** — the lock records the
crate's own version, and the release builds with `--locked`, so a bump without a
refreshed lock produces a tag that cannot be built at all. `v0.4.1` and `v0.6.2` were
lost that way before a preflight check existed to catch it:

```bash
cargo check                      # refreshes Cargo.lock after the bump
git commit -am "Release v0.7.2"
git tag v0.7.2 && git push origin master --tags
```

`.github/workflows/release.yml` first runs a preflight that rejects a tag disagreeing
with `Cargo.toml` or a stale lock, then builds macOS (universal), Linux x86_64 and Linux
arm64, verifies each binary runs, and publishes the tarballs with checksums. CI
(`ci.yml`) checks the same lock invariant on every push, so this should never reach a
tag.

Remember that a version bump invalidates every cached score, deliberately — the tool
version is part of the cache key so that a scoring change cannot be served from a
stale cache. Bump it whenever analysis behaviour changes, not only for releases.

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
| Duplicated Block | a run of ≥ 3 statements (≥ 30 tokens) repeated elsewhere in the file |

Test files only:

| Rule | Fires when |
| --- | --- |
| Large Assertion Blocks | > 8 consecutive assertions |
| Duplicated Assertion Blocks | the same assertion block appears ≥ 2 times |

### Observations — reported, never scored

Some things are worth telling you about but must not move the number.

| Observation | Fires when |
| --- | --- |
| Cross-File Duplication | the same function, structurally identical, in ≥ 2 files (≥ 5 LoC) |

Every point the score deducts traces to one measurement inside one file. That is
what makes a file's report a pure function of its own bytes, and `gate`, `delta`
and the cache all rest on it. Pricing a cross-file fact would make your score
depend on other files' contents and break that.

But leaving it unmeasured is worse. The scored `Code Duplication` rule only
compares functions *within* one file, so splitting a file that held two copies
deletes the finding and **raises** the score for a change that made the code
worse. For a metric built to be optimised against, a gradient pointing the wrong
way is a defect.

So observations are reported everywhere the score is — the `scan` summary,
`check` output, `--json`, `--guidance` — and resolved the same way a finding is:
fix it, or dismiss it with a reason.

```ts
// @aqs(disable:"Cross-File Duplication") the two hooks must not depend on each other
function reportFailure(step: string, error: unknown) {
```

A dismissed observation stays in the report with its reason attached, exactly as
a suppressed finding does. The count of *open* observations is the stopping
condition — reach zero by fixing or by explaining, and the record of which you
chose stays in the code.

Matching is exact on normalised tokens: identifiers and literals are collapsed,
so `confirmAction` and `confirmThing` match, but an 87%-similar pair does not.
Test files are skipped — suites repeat their setup on purpose, and
`Duplicated Assertion Blocks` is the test-side question.

`gate` prints observations a branch introduced and never fails on them. That is
not a half-measure: the branch this catches — one that copies a function into a
second file — moves no score at all, so the only run it would ever appear on is
a passing one. Gating on it would price the observation through the back door.
An observation already in the baseline is not reported again, and dismissing one
counts as resolving it.

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
- **Interfaces are exempt from Low Cohesion.** ABCs, Protocols, all-stub classes
  and a class declared `abstract` in TypeScript, Java or PHP are contracts for
  something else to fill in. An abstract class is still a class for every other rule. Flagging them would be telling you
  to stop declaring interfaces.
- **Test classes are exempt from cohesion and size rules.** Independent test
  methods are the goal. See `test_exempt_rules`.
- **Rust `?` is not a branch.** It is linear error propagation, not a decision a
  reader has to hold in mind, so counting it would penalise idiomatic Rust for being
  explicit about failure. `match` arms *are* counted, one each.
- **Rust `impl Trait for Type` is exempt from cohesion**, because the method set is
  dictated by the trait rather than chosen by the author — the same reasoning as
  abstract bases and `super()`-only overrides. Inherent `impl` blocks are judged.
- **Go's `if err != nil { return err }` is not a branch**, for the same reason Rust's
  `?` is not: it is the failure path going straight back out, not a decision the reader
  holds in mind. Go just has no sugar to hide it, and counting it would judge a function
  for how many things it calls that can fail. The exemption is narrow — no `else`, and
  the body must end in `return`. An `if err != nil` that recovers and carries on *is* a
  decision and still counts.
- **A field reached by bare name is still a field.** Java, PHP and Ruby all let a method
  read state without spelling out a receiver — `amount`, not `this.amount` — and the
  class's own field list is the only thing separating that from a local variable. Without
  it, a value object of six getters reads as six unrelated responsibilities.
- **Java's `default` arm is not a decision**, and neither is PHP's `default =>`. Both
  languages write it with the same node as a real `case`, so the exclusion TypeScript
  gets from its grammar has to be made by hand.
- **A class of `static` helpers is a namespace, not an object.** It holds no instance
  state, so cohesion has nothing to judge — the same reasoning that exempts Ruby's
  `def self.parse` and a Python `@staticmethod`.
- **PHP markup is not code.** A `.php` file may be HTML with code islands in it, and a
  line this tool cannot parse is a line it cannot explain a finding about, so it is not
  counted. Blade templates are excluded outright, for the same reason a Vue
  `<template>` is.
- **Ruby reads `@field` and `attr_reader` as the same state.** A class that goes
  through its own generated accessors -- `attr_reader :options`, then a bare `options` --
  shares just as much state as one writing `@options`, and LCOM4 has to see both or
  every method in such a class reads as its own responsibility.
- **Ruby `def self.parse` is a namespace function, not a method.** It holds no instance
  state, so cohesion must not count it as a responsibility — the same reasoning that
  exempts a Python `@staticmethod` and a Rust associated function.
- **A Ruby namespace module is not the class it wraps.** Methods belong to their
  *nearest* enclosing class, so `module Billing` wrapping a class is not judged for
  methods it did not write.
- **A trailing `if`/`unless` does not nest.** `return nil unless valid?` is a guard
  clause, and the whole reason to write it that way is that it does not indent the
  method. It still counts as a branch.
- **Go has no class rules.** Nothing in Go's syntax owns a type *and* its methods — a
  method is a top-level declaration that names its receiver — so there is no class scope
  to measure cohesion, size or method count over. Reading `type X struct` as the class
  would report every type as having no methods, which is worse than saying nothing.
- **Go's `NewThing(..)` is a constructor**, by the only convention the language has. The
  capital after `New` is what keeps `Newlines` an ordinary function.
- **Go parameters sharing a type count one each.** `func f(a, b, c int)` is a single
  node in the grammar, but the caller still passes three arguments.
- **Pure accessors are exempt from cohesion.** One statement, no branching and no
  calls, touching a field: a value object with six getters is not six unrelated jobs.
  The no-calls condition is load-bearing — `self.orders.append(o)` calls something, so
  a class appending to two unrelated collections is still correctly condemned.

- **Vendored code is excluded by default**, including the shadcn/ui directory —
  those are generated primitives whose near-identical thin `forwardRef` wrappers are
  that library's intended design, not duplication to fix. shadcn is configurable, so
  the location is read from `components.json` (`aliases.ui`) rather than assumed;
  both `components/ui` and `shared/ui` are also static defaults, since a scan of a
  subdirectory may never see `components.json`.
- **Path patterns match whole paths *and* individual segments.** Without that, a
  directory pattern like `*/e2e/*` needs a leading segment and misses `e2e/flow.ts` —
  where Playwright actually puts its specs — so the same file would count as test code
  or production code depending on which directory you scanned from.
- **Linked git worktrees are skipped by default.** A worktree is a second checkout of
  the same repository, so scanning one scores every file twice: it skews the
  LoC-weighted aggregate toward whichever branch is checked out there, and it makes
  every function collide with its own original in the cross-file join. On a real repo
  with five worktrees this was the difference between 523 files and 111, and between
  93 cross-file observations and none.

  It is detected, not guessed: a worktree root holds a `.git` *file* reading
  `gitdir: …/.git/worktrees/<name>`. A directory merely *named* `trees/` is ordinary
  source, and a submodule — same file form, but `…/.git/modules/<name>` — is part of
  the project and stays in scope.

  ```bash
  aqs scan .                       # worktrees skipped
  aqs scan . --include-worktrees   # or "scan_worktrees": true in .aqs.json
  aqs scan trees/epic-0/feature    # naming one directly always works
  ```

  `delta` and `gate` are unaffected — they read files out of git revisions, where a
  worktree's contents never appear.

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
  "exclude_patterns": ["*/legacy/*"],
  "scan_worktrees": false
}
```

A rule's value can be a bare number (its weight, `0.0` disables it) or an object
overriding `base`, `scale`, `cap`, `file_cap` or `weight`.

`exclude_patterns`, `test_patterns` and `test_exempt_rules` **replace** their
defaults rather than adding to them. `scan_worktrees` opts back into scanning
linked git worktrees, which are skipped by default.

Run `aqs explain --thresholds` for the full list of threshold names.

### Overriding one finding, in the code

When the metric is wrong about a *specific* piece of code, say so at the site rather than
bending the code or disabling the rule everywhere:

```rust
// @aqs(disable:"Complex Conditional") the bounds check and the equality read better
// together than behind a predicate name
fn extend(&self, found: Match, window: Window) -> Match {
```

```python
# @aqs(disable:"Complex Method", disable:"Large Method") generated parser table
# @aqs(disable-all) rewritten in DEV-4127
```

A directive governs the **next function** below it, and works in any comment style
(`#`, `//`, `/* */`). For file-level rules, which have no function to annotate, use
`@aqs-file(...)` anywhere in the file:

```typescript
// @aqs-file(disable:"Lines of Code in a Single File") vendored, regenerated by codegen
```

The shape follows [CodeScene's code comment directives][cs-directives], with three
deliberate differences:

- **A reason is required.** An unexplained suppression is invisible debt — the same
  failure mode as a config typo that silently disables a rule, which this tool already
  treats as fatal. A directive with nothing after the `)` fails the scan.
- **A misspelled rule name is an error**, located by file and line. A directive that
  quietly does nothing is worse than no directive.
- **A broken directive can never lower a score.** The file is scored as though the
  directive were absent, and the CLI exits `2`. Fail-safe, not fail-open.

Suppressions are never silent. Every report counts them, and `--json` lists each one with
its reason and the directive's line, so a reviewer can audit exactly what a branch chose
to ignore:

```bash
aqs scan . --json | jq '.files[].suppressed[] | {category, function, reason}'
```

[cs-directives]: https://codescene.io/docs/guides/technical/code-health.html


## Performance

Measured on a 258,955 LoC mixed Python/TS corpus, 12-core M-series, all cores,
end-to-end through the CLI including JSON output.

| | Time | Throughput |
| --- | --- | --- |
| `scan`, 258,955 LoC | 0.76s | 342,694 LoC/s |
| `scan`, frontend repo with `node_modules` | 21ms | — |
| `check`, single file | 2.6ms | — |
| `explain` (startup only) | 2.2ms | — |

The ~2ms floor is the point of shipping a binary: a loop that scores a file after each
edit pays no interpreter or import cost at all. For reference, the Python
implementation this replaced measured 3.4x slower in bulk (116,747 LoC/s) and 12-14x
slower on small inputs, where its ~122ms of interpreter startup dominated everything.

Excluded directories are pruned during traversal rather than filtered file by file,
which matters more than it sounds: one real frontend checkout held 39,833 files under
`node_modules` against 78 files of source, and filtering leaves cost 630ms of the
650ms scan.

Full two-sided merge gate on the same corpus — it scores every file in two trees, so
this is the expensive case:

| | Time | 100k LoC equivalent |
| --- | --- | --- |
| `--no-cache` | 1.72s | 0.66s |
| cold cache | 1.29s | 0.50s |
| **warm cache** | **0.45s** | **0.17s** |

All produce an identical score. Caching and raw speed are independent levers that
compose: caching removes work, and what remains runs faster. A cold run already beats
an uncached one because both sides of a `HEAD`-vs-`HEAD` gate share blobs.

Analysis is parallel across cores via rayon, with results collected in input order,
so the report is byte-identical however many threads ran.

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

The `tree-sitter-*` grammar pins in `Cargo.toml` are **exact, not ranges**.
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

- **Fragmentation is mostly invisible.** Splitting a bad 300-line function across
  five files will raise the score without improving the design. `Number of
  Functions in a Single Module` catches within-module churning, and the
  Cross-File Duplication observation now catches the case where a split leaves
  identical copies behind — but nothing catches logic merely spread thinly across
  new files. If you point a model at this metric, review its diffs — do not just
  watch the number go up.
- **Repeated blocks are only seen within one file.** `Duplicated Block` covers
  fragments inside a single file and Cross-File Duplication covers whole
  functions across files, but a three-line block repeated across *different*
  files falls between them. Catching that needs a repository-wide block index,
  which is a different order of cost from the per-function hash join.
- **A grammar only parses the syntax it was pinned at.** The grammars are pinned
  exactly, because a bump can rename AST nodes and silently change every score — which
  means brand-new syntax parses as an error until the pin moves. Java 25's statements
  before `super()` is the live example: 3 of 950 JDK files hit it. A file with syntax
  errors is still scored, on the part that parsed, and reports its error count in
  `syntax_errors` rather than failing.
- **Dead code, unused exports and consistency are not measured.** Nothing here
  looks at reachability or at whether sibling modules resemble each other. A file
  full of functions nobody calls scores 10.00.
- **Cross-File Duplication is very sensitive to copied trees.** Any second copy of
  the same code inside the scan root — a vendored dependency, a backup directory,
  an unpacked archive — turns *every* function in it into an observation against
  its own original. Linked git worktrees are detected and skipped for exactly this
  reason (see below); anything else you have to exclude yourself:

  ```json
  { "exclude_patterns": ["*/vendor/*", "*/.backup/*"] }
  ```

  Note that `exclude_patterns` **replaces** the defaults rather than adding to
  them, so copy the list from `Configuration` before editing it.
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
- **One known grammar gap.** `f<typeof import("./m")>()` — an import type inside type
  arguments — is rejected by tree-sitter-typescript 0.23.2, the newest published
  crate. It surfaced in 3 files across 25 real repositories, all `vi.mock` calls in
  tests. The error is localised and verified inert: substituting an equivalent type
  argument leaves the score, LoC and finding set unchanged. Files affected report a
  non-zero `syntax_errors` count so you can tell.

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

## Scoring itself

`aqs` scores this repository, with **no `.aqs.json` and no exempted rules**:

```
aqs scan src tests examples        10.00 / 10   49 files · 5,594 LoC · 0 findings
```

It did not start there. The first self-scan was **6.23**, and the findings were real:

| | Finding | Fix |
| --- | --- | --- |
| `load_profile` | Brain Method — 90 lines, cc 21, nesting 5 | four named steps |
| `run` | 190 lines, cc 26 | one handler per subcommand, each taking an `Args` struct |
| `constructor_over_injection` | 98% similar to `excess_arguments` | one `argument_count` detector |
| `find_longest_match` | cc 14, nesting 4 | DP row extracted; parity tests unchanged |
| `compute_delta`, `compute_gate` | six arguments each | `DeltaRequest`, `GateRequest` |
| difflib's `(alo, ahi, blo, bhi)` | four bare indices | a `Window` of two ranges |
| `Profile`'s pattern predicates | Low Cohesion | matching moved onto `PatternSet`, where it belongs |
| `metrics.rs`, `detectors.rs`, `render.rs`, `profile.rs`, `cache.rs`, `main.rs` | 21-54 functions each, 560-660 lines | split into module directories along what each part measures |

The module split is the interesting one, because it resolved what looked like a wall.
**Two rules pull against each other**: Bumpy Road and Large Method ask you to extract
functions, and Number of Functions counts them — extracting `function_name` into three
helpers removed one finding and added another. That tension is real, but it is *local*.
Splitting the module satisfies both at once, and the resulting layout is easier to
navigate than the 735-line original. There was no wall, only a reorganisation I had been
avoiding.

Four things worth knowing about the result:

- **One finding is suppressed, in the code, with a reason.** `extend` in `difflib.rs`
  carries an `@aqs(disable:"Complex Conditional")` directive: inlining the
  bounds-and-equality checks into the two loop conditions reads better than hiding them
  behind predicates. The report says `1 finding(s) suppressed by @aqs directives` every
  time it runs, which is the point — the disagreement is recorded where the code is, and
  it stays visible rather than becoming a README footnote nobody reads.
- **Nothing is exempted.** An earlier version of this section justified four disabled
  rules. All four turned out to be reachable by refactoring, so the config is gone.
- **The refactor is verified behaviour-preserving.** Every change was checked by scoring
  nine real repositories with the previous release and diffing the findings, not just the
  scores. Two regressions surfaced that way and were fixed: excluding accessors from
  *connectivity* as well as from counting could raise LCOM4 by removing the link that held
  a class together, and a `super()` delegation that computes an argument
  (`super().update(deleted_at=timezone.now())`) was no longer recognised as a delegation,
  which flagged a perfectly cohesive Django QuerySet.
- **Fidelity to CPython is guaranteed by tests, not by structure.** `difflib.rs` pins
  exact CPython return values, including the autojunk threshold. Those tests are what make
  the port safe to refactor; mirroring CPython's function layout would not have added
  anything, and its `find_longest_match` is 115 lines besides.

If you point a model at this metric, the lesson is the one above: prefer the
reorganisation you have been avoiding to a contorted local refactor, and verify against a
real corpus rather than against the score.

## Provenance

This started as a Python implementation, which was then ported to Rust and verified
byte-identical across 1,191 files and 3,841 findings — every score, penalty,
`details` string and threshold. The Python version was removed once parity held; it
lives in git history if you ever want the oracle back:

```bash
git log --diff-filter=D --name-only -- 'src/aqs/*'   # find the removal commit
git show <commit>^:src/aqs/detectors.py              # read any file as it was
```

Three details had to be ported *exactly* rather than reasonably. They are worth
knowing before touching the corresponding code, because each silently changes scores:

- **`difflib.SequenceMatcher`** (`src/difflib.rs`) is a deliberate port of CPython's
  algorithm, autojunk heuristic included: above 200 elements it drops any element
  occurring in more than 1% of the input. Any "sensible" substitute — Levenshtein,
  Jaccard, a different LCS — gives different ratios and moves the duplication
  threshold. The unit tests pin exact CPython values.
- **`fnmatch` semantics** (`src/glob.rs`), where `*` matches `/`. Swapping in a
  path-aware glob crate would silently break every exclude pattern in the profile.
- **Detector emission order** (`src/detectors.rs`), because per-rule cap allocation
  breaks ties by discovery order, so a different traversal changes which findings get
  trimmed when a cap binds.

Two real bugs surfaced only because there was a reference to disagree with: an
unanchored regex in the glob translation (`test_*.py` matched `x_test_a.py`), and a
`lstrip("await ")` that stripped a *character set* rather than a prefix, mangling
`assertEqual(...)` into `ssertEqual(...)`.

## Development

```bash
make setup                    # once per clone: the coverage tool, and git's hooks in .githooks
make check-fast               # lock file, rustfmt, clippy, and this repo's own score (must be 10)
make check                    # the above, plus the 397 tests at 100% coverage
cargo test --test 'cli*'      # the 106 that drive the real binary
```

The checks are defined once, in the `Makefile`, and everything else calls it:

| | runs |
| --- | --- |
| `make check-fast` | before every commit, and when Claude Code wants to end a turn with changes in the tree (`.claude/hooks/check-on-stop.sh`) |
| `make check` | before every push, on every push in CI, and in the release preflight before anything is built |

The tool gates its own source at exactly 10. A finding that is right to leave in place
gets an `@aqs` directive with its reason, which stays visible in every report; lowering
the threshold is not the way out. A fresh clone is not set up until `make setup` has run.

`make check` runs the tests under [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)
and fails below 100% of lines or 100% of functions, closures included. The runs of the
real binary that the CLI suites start are counted. A line no test executes is listed by
file and number; the way out is a test, or deleting the line if nothing can reach it.
Stable Rust has no way to exempt a line, and that is deliberate here.

Coverage says a line ran, not that a test would notice it being wrong. For that there is
[`cargo-mutants`](https://mutants.rs), which changes the source one operator or return
value at a time and runs the suite against each change:

```bash
cargo install cargo-mutants --locked
cargo mutants                 # about 1,350 mutants; an hour on eight cores, split with --shard
cargo mutants -f src/glob.rs  # one file, in a few minutes
```

None survives. It is too slow for `make check`, so it is run by hand after a change to
the measuring code. A mutant no test can tell from the original is either written out of
the code or listed in `.cargo/mutants.toml` with the reason; there is one. The dozen that
time out are loops that no longer end, which is its own way of being caught.

Three kinds of suite, and the split is deliberate. The CLI one is spread over six files
by subject, sharing `tests/common`; the behavioural ones share `tests/support`:

| suite | covers |
| --- | --- |
| unit tests in `src/` | measurement primitives, glob and difflib parity, config validation |
| `tests/behaviour.rs` | one test per rule — that it fires, and that it stays quiet on correct code |
| `tests/behaviour_languages.rs` | the same rules read through Rust, Go, Ruby, Java and PHP |
| `tests/boundaries.rs` | each limit met exactly and passed by one, on a profile that sets it to the sample's own measure |
| `tests/boundaries_classes.rs` | which methods count towards a class's cohesion, and which classes are let off |
| `tests/cli.rs` | the binary itself: exit codes, JSON shape, git-backed `delta` and `gate` |
| `tests/cli_directives.rs` | `@aqs` directives through the binary: what they suppress, how a bad one fails |
| `tests/cli_duplication.rs` | cross-file observations, `Duplicated Block`, and what the gate says a branch introduced |
| `tests/cli_discovery.rs` | what a scan picks up: worktrees, single-file components, each language |
| `tests/cli_edges.rs` | the flags and wordings one step from the usual path: `--no-cache`, `--no-merge-base`, widths, what a clean run leaves out |
| `tests/cli_reports.rs` | what the reports say to a person: caveats, observations, `explain` listings, how `delta` and `gate` word a change |

The CLI suite exists because the CLI *is* the contract — exit codes gate automation and
the JSON shape is what a model consumes. It was previously verified only by hand, and two
regressions slipped through that way: an error printed whenever `--min-score` was passed
rather than only on failure, and `.aqs.json` silently ignored for a bare-filename target.
Both are now pinned. No test mutates the process working directory; where one needs to run
"inside" a directory it sets the *child's* cwd, so the suite is safe under `cargo test`'s
default parallelism.

The architecture keeps detection and scoring strictly separate:

| Module | Responsibility |
| --- | --- |
| `languages` | per-language AST vocabulary, one module each — add a language here |
| `sfc` | reduces a Vue or Svelte single-file component to its script, in place |
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
| `difflib` | CPython-compatible similarity, for Code Duplication |
| `glob` | `fnmatch`-compatible pattern matching, for the path patterns |

Adding a rule means writing one detector function with a `@function_detector` or
`@file_detector` decorator, one `RuleConfig` entry, and one `RuleDoc`. Because no
detector knows about points, the penalty model stays swappable — a future
`--profile=codescene` fitted against real `cs review` output would not require
touching a single detector.
