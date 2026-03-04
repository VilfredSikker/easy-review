#!/usr/bin/env bash
#
# Validate a base branch ref, capture the diff, and compute its SHA-256 hash.
#
# Usage: scripts/er-freshness-check.sh <base-branch> [extra-diff-args...]
# Output (3 lines):
#   Line 1: "ok" if base branch is valid (exits non-zero otherwise)
#   Line 2: <hash>  .er-diff-tmp   (shasum output)
#   Line 3: <head-commit-short>
#
# Side effect: writes .er-diff-tmp in cwd (gitignored)
#
# Examples:
#   scripts/er-freshness-check.sh main
#   scripts/er-freshness-check.sh main --unified=3 --no-color --no-ext-diff

set -euo pipefail

BASE="$1"
shift

# Validate base branch is a real ref (prevents injection from crafted .er-review.json)
if ! git rev-parse --verify "$BASE" >/dev/null 2>&1; then
  echo "error: invalid base branch '$BASE'" >&2
  exit 1
fi
echo "ok"

# Capture diff and hash
git diff "$BASE" --unified=3 --no-color --no-ext-diff "$@" > .er-diff-tmp
shasum -a 256 .er-diff-tmp

# Current commit
git rev-parse --short HEAD
