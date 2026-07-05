#!/usr/bin/env bash
# Clone the evaluation corpus into eval/corpus/ (gitignored).
#
# Usage: eval/clone-corpus.sh [tier|name ...]
#   No arguments: clone every repo in corpus.tsv.
#   Arguments filter by tier ("1") or repo name ("requests"); mix freely.
#
# Reproducibility: the first successful clone of a repo records its commit in
# corpus.lock (committed); later clones on any machine check out that exact
# commit, so every run analyzes identical input. Delete a repo's line from
# corpus.lock to re-pin it to the current upstream HEAD.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
corpus_dir="$here/corpus"
tsv="$here/corpus.tsv"
lock="$here/corpus.lock"
filters=("$@")

mkdir -p "$corpus_dir"
touch "$lock"

selected() {
  local tier="$1" name="$2" f
  [ ${#filters[@]} -eq 0 ] && return 0
  for f in "${filters[@]}"; do
    if [ "$f" = "$tier" ] || [ "$f" = "$name" ]; then
      return 0
    fi
  done
  return 1
}

while IFS=$'\t' read -r tier name url; do
  case "$tier" in '' | '#'*) continue ;; esac
  selected "$tier" "$name" || continue
  dest="$corpus_dir/$name"
  if [ -e "$dest/.git" ]; then
    echo "skip   $name (already cloned)"
    continue
  fi
  pinned="$(awk -F'\t' -v n="$name" '$1 == n { print $2 }' "$lock")"
  if [ -n "$pinned" ]; then
    echo "clone  $name @ ${pinned:0:12} (locked)"
    git init -q "$dest"
    git -C "$dest" remote add origin "$url"
    git -C "$dest" fetch -q --depth 1 origin "$pinned"
    git -C "$dest" checkout -q FETCH_HEAD
  else
    echo "clone  $name @ HEAD (pinning into corpus.lock)"
    git clone -q --depth 1 "$url" "$dest"
    sha="$(git -C "$dest" rev-parse HEAD)"
    printf '%s\t%s\n' "$name" "$sha" >>"$lock"
  fi
done <"$tsv"

sort -o "$lock" "$lock"
echo "corpus ready under $corpus_dir"
