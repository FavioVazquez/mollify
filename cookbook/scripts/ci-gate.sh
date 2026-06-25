#!/usr/bin/env bash
# Mollify cookbook — "ratchet" CI gate.
# Fails only when a pull request introduces a NEW finding, ignoring the
# project's pre-existing debt. Drop this into CI on a legacy codebase to stop
# the bleeding without a big-bang cleanup.
#
# Usage:  ./cookbook/scripts/ci-gate.sh [PROJECT_DIR]
#   PROJECT_DIR defaults to the bundled cookbook/sample-project.
#
# How it works:
#   * First run (no baseline)  -> snapshot today's findings, pass.
#   * Later runs               -> report only fingerprints new since the
#                                 baseline; exit non-zero if any appeared.
# Commit the baseline file to your repo and refresh it intentionally when you
# pay down debt.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROJECT="${1:-$SCRIPT_DIR/../sample-project}"
BASELINE="$PROJECT/.mollify/baseline.json"

if command -v mollify >/dev/null 2>&1; then
  MOLLIFY="mollify"
elif [[ -x "$REPO_ROOT/target/release/mollify" ]]; then
  MOLLIFY="$REPO_ROOT/target/release/mollify"
else
  echo "Could not find 'mollify' (install it, or 'cargo build --release')." >&2
  exit 1
fi

if [[ ! -f "$BASELINE" ]]; then
  echo "No baseline yet — creating one from the current state:"
  "$MOLLIFY" audit --path "$PROJECT" --save-baseline "$BASELINE"
  echo "Baseline written to $BASELINE. Commit it, then re-run to enforce."
  exit 0
fi

echo "Gating against baseline: $BASELINE"
echo "(only findings NEW since the baseline will fail the build)"
echo
exec "$MOLLIFY" audit --path "$PROJECT" \
  --baseline "$BASELINE" \
  --fail-on-regression
