#!/usr/bin/env bash
# Advisory Mollify audit for agent hooks. Runs a deterministic audit and prints a
# short summary. Non-blocking: it surfaces findings but never fails the action
# (the blocking PR-gate `--gate new-only` is a planned upgrade — see docs/STATUS.md).
#
# Used by both Cascade (.windsurf/hooks.json) and Devin/Claude (.devin/hooks.v1.json).
set -euo pipefail

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

REPORT="$("$BIN" audit --format json 2>/dev/null || true)"
[ -z "$REPORT" ] && exit 0

if command -v jq >/dev/null 2>&1; then
  TOTAL=$(printf '%s' "$REPORT" | jq -r '.summary.total // 0')
  CERTAIN=$(printf '%s' "$REPORT" | jq -r '[.findings[]? | select(.confidence=="certain")] | length')
  [ "${TOTAL:-0}" -eq 0 ] && exit 0
  echo "mollify: ${TOTAL} finding(s), ${CERTAIN} high-confidence. Top items:"
  printf '%s' "$REPORT" | jq -r \
    '[.findings[]? | select(.confidence=="certain")][:5][] | "  \(.location.path):\(.location.line) \(.rule) — \(.reason)"'
else
  echo "mollify: audit complete (install jq for a detailed summary)."
fi
exit 0
