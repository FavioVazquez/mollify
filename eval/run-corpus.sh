#!/usr/bin/env bash
# Run every mollify analysis engine over the cloned corpus.
#
# Usage: eval/run-corpus.sh [run-label] [repo-name ...]
#   run-label names the output subdirectory (default "run1"); use a second
#   label (e.g. "run2") for the determinism re-run, then diff -r the two.
#   Repo names filter which corpus clones to analyze (default: all cloned).
#
# Output per repo/engine under eval/results/<label>/<repo>/ (gitignored):
#   <engine>.json    the kind-discriminated JSON report (stdout)
#   <engine>.stderr  anything the engine wrote to stderr
#   <engine>.meta    exit code, wall seconds, peak RSS kB (from GNU time)
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
bin="$root/target/release/mollify"
label="${1:-run1}"
shift || true
repos=("$@")

[ -x "$bin" ] || { echo "error: build first: cargo build --release" >&2; exit 1; }

engines=(audit dead-code deps arch complexity dupes types security)

for dir in "$here"/corpus/*/; do
  name="$(basename "$dir")"
  if [ ${#repos[@]} -gt 0 ]; then
    keep=1
    for r in "${repos[@]}"; do [ "$r" = "$name" ] && keep=0; done
    [ $keep -eq 0 ] || continue
  fi
  out="$here/results/$label/$name"
  mkdir -p "$out"
  for engine in "${engines[@]}"; do
    /usr/bin/time -f 'wall_s %e\nmax_rss_kb %M' -o "$out/$engine.meta" \
      "$bin" "$engine" --path "$dir" --format json \
      >"$out/$engine.json" 2>"$out/$engine.stderr"
    code=$?
    printf 'exit_code %s\n' "$code" >>"$out/$engine.meta"
    printf '%-18s %-12s exit=%s  %s\n' "$name" "$engine" "$code" \
      "$(head -c 120 "$out/$engine.stderr" | tr '\n' ' ')"
  done
done
echo "results under $here/results/$label"
