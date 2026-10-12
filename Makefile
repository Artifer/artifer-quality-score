# The deterministic checks — the one place they are defined.
#
#   make setup       install the coverage tool and point git at the hooks in .githooks
#   make check-fast  seconds — the pre-commit hook, and Claude before it ends a turn
#   make check       adds the tests, at 100% coverage — the pre-push hook, CI, and the
#                    release preflight
#
# check includes check-fast. The git hooks, the Claude hook and the workflows call these
# targets and never list the tools themselves. A step that fails stops the run.

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.PHONY: setup tools check-fast check

# Pinned: a newer cargo-llvm-cov may count lines differently, and a check that answers
# differently on two machines is no gate.
LLVM_COV_VERSION := 0.9.1

STEP = @printf '\n── %s\n'

setup: tools
	git config core.hooksPath .githooks

# What `make check` needs beyond a Rust toolchain. Also the first step in CI.
tools:
	rustup component add llvm-tools
	@cargo llvm-cov --version 2>/dev/null | grep -q " $(LLVM_COV_VERSION)$$" \
		|| cargo install cargo-llvm-cov --locked --version $(LLVM_COV_VERSION)

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
	$(STEP) "tests, 100% line and function coverage"
	@# The CLI suites drive the real binary, and its runs are counted too. A line no test
	@# executes is listed by file and number.
	cargo llvm-cov --locked --summary-only --show-missing-lines \
		--fail-under-lines 100 --fail-under-functions 100
