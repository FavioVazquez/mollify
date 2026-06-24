#!/usr/bin/env bash
# Fail if the release version disagrees anywhere it must match the workspace
# version: internal crate dep constraints and the npm meta package (version +
# every optionalDependencies entry). Run by CI; fix with scripts/bump-version.sh.
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

# 2. npm meta package.
WS="$WS" node - "$ROOT/npm/mollify/package.json" <<'NODE' || fail=1
const fs = require("fs");
const ws = process.env.WS;
const j = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
let bad = false;
if (j.version !== ws) { console.error(`MISMATCH (npm version): ${j.version} (want ${ws})`); bad = true; }
for (const [k, v] of Object.entries(j.optionalDependencies || {})) {
  if (v !== ws) { console.error(`MISMATCH (npm optionalDependency ${k}): ${v} (want ${ws})`); bad = true; }
}
process.exit(bad ? 1 : 0);
NODE

if [[ "$fail" -ne 0 ]]; then
  echo "Version drift detected — run scripts/bump-version.sh $WS" >&2
  exit 1
fi
echo "All version references agree ($WS)."
