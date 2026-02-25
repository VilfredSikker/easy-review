#!/usr/bin/env bash
#
# Compute per-file SHA-256 hashes from a git diff.
#
# Usage: scripts/er-hash-files.sh <scope-args...>
# Output: <file>\t<hash>  .er-diff-tmp   (one line per changed file)
#
# Examples:
#   scripts/er-hash-files.sh main --unified=3 --no-color --no-ext-diff
#   scripts/er-hash-files.sh --unified=3 --no-color --no-ext-diff
#   scripts/er-hash-files.sh --staged --unified=3 --no-color --no-ext-diff

set -euo pipefail

git diff "$@" > .er-diff-tmp

current_file=""
current_section=""

while IFS= read -r line; do
  if [[ "$line" == "diff --git a/"* ]]; then
    if [[ -n "$current_file" ]]; then
      hash=$(printf '%s' "$current_section" | shasum -a 256 | cut -d' ' -f1)
      printf "%s\t%s  .er-diff-tmp\n" "$current_file" "$hash"
    fi
    current_file="${line#diff --git a/}"
    current_file="${current_file%% b/*}"
    current_section="$line"$'\n'
  else
    current_section+="$line"$'\n'
  fi
done < .er-diff-tmp

if [[ -n "$current_file" ]]; then
  hash=$(printf '%s' "$current_section" | shasum -a 256 | cut -d' ' -f1)
  printf "%s\t%s  .er-diff-tmp\n" "$current_file" "$hash"
fi
