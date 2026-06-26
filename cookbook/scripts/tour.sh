#!/usr/bin/env bash
# Mollify cookbook — guided tour.
# Runs the headline commands against cookbook/sample-project/ with narration.
# Usage:  ./cookbook/scripts/tour.sh
set -euo pipefail

# --- locate the mollify binary: PATH first, then a local source build ---------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SAMPLE="$SCRIPT_DIR/../sample-project"

if command -v mollify >/dev/null 2>&1; then
  MOLLIFY="mollify"
elif [[ -x "$REPO_ROOT/target/release/mollify" ]]; then
  MOLLIFY="$REPO_ROOT/target/release/mollify"
else
  echo "Could not find 'mollify'. Install it (uvx/pipx mollify, or cargo install mollify-cli) or run" >&2
  echo "'cargo build --release' from the repo root, then re-run this script." >&2
  exit 1
fi

cyan() { printf '\033[36m%s\033[0m\n' "$*"; }
dim()  { printf '\033[2m%s\033[0m\n' "$*"; }
step() { echo; cyan "━━━ $1"; dim "\$ $2"; echo; }

cd "$SAMPLE"

cyan "Mollify cookbook tour — analyzing cookbook/sample-project/"
dim  "binary: $MOLLIFY"

step "1. Full audit — every engine, one quality score" "mollify audit"
"$MOLLIFY" audit

step "2. Dead code only — what's safe to delete?" "mollify dead-code --min-confidence certain"
"$MOLLIFY" dead-code --min-confidence certain

step "3. Preview safe auto-fixes (dry-run, writes nothing)" "mollify fix"
"$MOLLIFY" fix

step "4. Dependency hygiene" "mollify deps"
"$MOLLIFY" deps

step "5. Complexity hotspots (cyclomatic + cognitive)" "mollify complexity"
"$MOLLIFY" complexity

step "6. Import graph (Mermaid)" "mollify graph --mermaid"
"$MOLLIFY" graph --mermaid

step "7. Explain a rule" "mollify explain unused-export"
"$MOLLIFY" explain unused-export

step "8. JSON contract — quality score via jq" "mollify audit --format json | jq .quality_score"
if command -v jq >/dev/null 2>&1; then
  "$MOLLIFY" audit --format json | jq '.quality_score'
else
  dim "(install jq to see this; printing raw summary instead)"
  "$MOLLIFY" audit --format json | head -c 200; echo
fi

echo
cyan "Done. Next: point it at your own code →  mollify audit --path /your/project"
