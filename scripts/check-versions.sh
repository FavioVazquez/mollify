#!/usr/bin/env bash
# Fail if the release version disagrees anywhere it must match the workspace
# version: the internal crate dep constraints. Run by CI; fix with
# scripts/bump-version.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

WS="$(sed -nE 's/^version = "([0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
if [[ -z "$WS" ]]; then
  echo "could not read workspace version from Cargo.toml" >&2
  exit 1
fi
echo "workspace version: $WS"

fail=0

# 1. Internal dep version constraints.
while IFS= read -r line; do
  ver="$(sed -nE 's/.*version = "([0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?)".*/\1/p' <<<"$line")"
  if [[ -n "$ver" && "$ver" != "$WS" ]]; then
    echo "MISMATCH (crate dep): $line  (want $WS)" >&2
    fail=1
  fi
done < <(grep -rhn 'path = "\.\./mollify-' "$ROOT"/crates/*/Cargo.toml)

if [[ "$fail" -ne 0 ]]; then
  echo "Version drift detected — run scripts/bump-version.sh $WS" >&2
  exit 1
fi
echo "All version references agree ($WS)."
