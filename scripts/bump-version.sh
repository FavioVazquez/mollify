#!/usr/bin/env bash
# Set the release version everywhere it must agree, in one shot:
#   - the workspace version           (Cargo.toml -> [workspace.package].version)
#   - internal crate dep constraints  (crates/*/Cargo.toml -> path+version deps)
# `pyproject.toml` reads the version from Cargo automatically (maturin), so it
# needs no edit.
#
# Usage: scripts/bump-version.sh 0.2.0
# Then:  scripts/check-versions.sh   (CI runs this to enforce agreement)
set -euo pipefail

VERSION="${1:-}"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?$ ]]; then
  echo "usage: bump-version.sh <semver>   e.g. 0.2.0" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Portable in-place sed: GNU sed wants `-i`, BSD/macOS sed wants `-i ''`.
sed_inplace() {
  if sed --version >/dev/null 2>&1; then
    sed -i -E "$1" "$2"        # GNU sed
  else
    sed -i '' -E "$1" "$2"     # BSD/macOS sed
  fi
}

# 1. Workspace version (the only line-anchored `version = "x.y.z"`).
sed_inplace "s/^version = \"[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?\"/version = \"$VERSION\"/" \
  "$ROOT/Cargo.toml"

# 2. Internal dep version constraints: `path = "../mollify-x", version = "x.y.z"`.
find "$ROOT/crates" -name Cargo.toml -print0 | while IFS= read -r -d '' f; do
  sed_inplace "s|(path = \"\.\./mollify-[a-z]+\", version = \")[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?(\")|\1$VERSION\3|g" "$f"
done

echo "Bumped all version references to $VERSION"
echo "Verify with: scripts/check-versions.sh"
