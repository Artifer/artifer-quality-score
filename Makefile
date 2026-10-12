# The deterministic checks — the one place they are defined.
#
#   make setup       point git at the hooks in .githooks
#   make check-fast  seconds — the pre-commit hook, and Claude before it ends a turn
#   make check       adds the tests — the pre-push hook, CI, and the release preflight
#
# check includes check-fast. The git hooks, the Claude hook and the workflows call these
# targets and never list the tools themselves. A step that fails stops the run.

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.PHONY: setup check-fast check

STEP = @printf '\n── %s\n'

setup:
	git config core.hooksPath .githooks

check-fast:
	$(STEP) "Cargo.lock is in sync"
	@# The release build runs with --locked; a stale lock is caught here, not on a tag.
	cargo metadata --locked --format-version 1 > /dev/null
	$(STEP) "rustfmt"
	cargo fmt --check
	$(STEP) "clippy"
	cargo clippy --all-targets --locked -- -D warnings
	$(STEP) "Artifer quality score of this repository (must be 10)"
	@# The debug build: the score does not depend on the profile, and it links in seconds.
	cargo run --quiet --locked -- scan . --no-cache --min-score 10

check: check-fast
	$(STEP) "tests"
	cargo test --locked
