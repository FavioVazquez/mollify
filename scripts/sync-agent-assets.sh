#!/usr/bin/env bash
# Mirror the canonical agent artifacts (repo root) into the mollify-core crate.
#
# Why: `mollify init --agent ...` embeds these files into the binary via
# `include_dir!`. To keep the published crate self-contained (so `cargo install
# mollify-cli` / crates.io and the maturin/npm builds all work identically),
# the embedded copy must live INSIDE the crate, not at `../../`. This script
# regenerates that in-crate copy from the repo-root canonical sources.
#
# Run this whenever you edit any of the agent artifacts, then commit the result.
# CI enforces that the copy is in sync via the `assets_match_repo_root_sources`
# test in `crates/mollify-core/src/agents.rs`.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/crates/mollify-core/assets"

# Canonical agent artifact roots (dirs + marker/helper files). May be nested
# paths (e.g. the hook helper under scripts/).
ITEMS=(
  .claude
  .cursor
  .gemini
  .codex
  .agents
  .devin
  .windsurf
  .mcp.json
  GEMINI.md
  AGENTS.md
  scripts/mollify-report.sh
)

rm -rf "$DEST"
mkdir -p "$DEST"
for item in "${ITEMS[@]}"; do
  if [ -e "$ROOT/$item" ]; then
    mkdir -p "$(dirname "$DEST/$item")"
    cp -R "$ROOT/$item" "$DEST/$item"
  else
    echo "warning: missing canonical source $item" >&2
  fi
done

echo "Synced ${#ITEMS[@]} agent artifact roots into $DEST"
