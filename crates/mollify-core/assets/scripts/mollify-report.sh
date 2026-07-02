#!/usr/bin/env bash
# Advisory Mollify audit for agent hooks. Runs a deterministic audit and prints a
# short summary. Non-blocking: it surfaces findings but never fails the action.
#
# Invoked relatively (`bash scripts/mollify-report.sh`) by the Claude
# (`.claude/settings.json`) and Cascade/Devin (`.windsurf/hooks.json`,
# `.devin/hooks.v1.json`) hooks. `mollify init --agent claude|cascade` ships this
# file alongside those hooks, so the path resolves on any machine.
set -euo pipefail

# Run from the project root so a relative invocation (or an odd hook CWD) still
# audits the whole project. Falls back to the current directory outside git.
ROOT_DIR="$(git rev-parse --show-toplevel 2>/dev/null || true)"
[ -n "$ROOT_DIR" ] && cd "$ROOT_DIR"

# Prefer an installed `mollify`; fall back to a local debug build when dogfooding.
if command -v mollify >/dev/null 2>&1; then
  BIN=mollify
elif [ -x "./target/debug/mollify" ]; then
  BIN="./target/debug/mollify"
elif [ -x "./target/release/mollify" ]; then
  BIN="./target/release/mollify"
else
  # Mollify not installed yet — stay silent and non-blocking.
  exit 0
fi

# Prefer newly-introduced findings (gate); falls back to all if not a git repo.
REPORT="$("$BIN" audit --gate new-only --format json 2>/dev/null || true)"
[ -z "$REPORT" ] && REPORT="$("$BIN" audit --format json 2>/dev/null || true)"
[ -z "$REPORT" ] && exit 0

if command -v jq >/dev/null 2>&1; then
  # Every jq call is guarded with a fallback: invalid/partial JSON must never
  # make this script exit nonzero (it runs under `set -euo pipefail` and is
  # wired into agent hooks where a nonzero exit blocks the action).
  TOTAL=$(printf '%s' "$REPORT" | jq -r '.summary.total // 0' 2>/dev/null || echo 0)
  CERTAIN=$(printf '%s' "$REPORT" | jq -r '[.findings[]? | select(.confidence=="certain")] | length' 2>/dev/null || echo 0)
  # Normalize anything non-numeric (e.g. jq output on malformed input) to 0.
  case "$TOTAL" in '' | *[!0-9]*) TOTAL=0 ;; esac
  case "$CERTAIN" in '' | *[!0-9]*) CERTAIN=0 ;; esac
  [ "$TOTAL" -eq 0 ] && exit 0
  echo "mollify: ${TOTAL} finding(s), ${CERTAIN} high-confidence. Top items:"
  printf '%s' "$REPORT" | jq -r \
    '[.findings[]? | select(.confidence=="certain")][:5][] | "  \(.location.path):\(.location.line) \(.rule) — \(.reason)"' \
    2>/dev/null || true
else
  echo "mollify: audit complete (install jq for a detailed summary)."
fi
exit 0
