#!/usr/bin/env bash
# Claude Code Stop hook: an agent is not done until `make check-fast` passes.
#
# Runs when Claude wants to end its turn. With changes in the working tree it runs the
# fast checks; a failure is handed back to Claude, which has to fix it before it can stop.
#
# One round only. When Claude tried and it is still red, the hook lets it stop and shows
# the failure to the person instead, so an unfixable check cannot keep an agent in a loop.
# The tests (`make check`) stay with the pre-push hook.

set -u
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0

event=$(cat)

# A clean working tree: the commit hook already ran.
[ -z "$(git status --porcelain 2>/dev/null)" ] && exit 0

output=$(make check-fast 2>&1) && exit 0
tail=$(printf '%s\n' "$output" | tail -40)

if printf '%s' "$event" | grep -Eq '"stop_hook_active"[[:space:]]*:[[:space:]]*true'; then
  printf 'make check-fast still fails after a fix attempt:\n%s\n' "$tail" >&2
  exit 1 # shown to the person
fi
printf 'make check-fast fails. Fix the code (never the check) and finish again.\n%s\n' "$tail" >&2
exit 2 # handed back to Claude
