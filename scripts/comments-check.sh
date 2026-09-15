#!/usr/bin/env bash
# Reject inline comments that only make sense at the moment they were written.
#
# The failure this exists for is not a typo — it is a comment that cites the
# change which produced it: a review-finding code, a plan step, a tracker
# number, a release merge. The reader has the code and nothing else, so none of
# it resolves. Nothing else notices, because the code still compiles. The same
# three review passes that cleaned these out by hand are why the shapes are
# known well enough to check.
#
# Deliberately high-precision, not exhaustive. Only tokens that are never
# meaningful in a comment become a failure. The prose shapes — "used to",
# "previously", "no longer" — are left out on purpose: "the head is no longer a
# descendant of the previously fetched commit" is a durable statement about the
# system, and no pattern tells that apart from the narrative kind. A noisy gate
# gets switched off, which is worse than no gate. Those still need a reader,
# which is what the Inline comments section of docs/agents/writing-docs.md is
# for. `PR #N` is left out for the same reason: it doubles as fixture text in
# test comments.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Tokens that never resolve for someone holding only the code.
PATTERNS='review-fix-loop|§[A-Z][0-9]+|plan O[0-9]|first-paint plan|release/v[0-9]|the merge commit|\([OJCRSBF][0-9][):,]|P[0-9]-[0-9]|Step [A-Z][0-9]:|Part [A-Z]:|issues? #[0-9]+'
# Two deliberate narrowings, both driven by a false positive the gate produced on
# its first run against the tree that motivated it:
#   - "the merge commit" and not "merge commits" — the plural is ordinary git
#     vocabulary ("handling merge commits and root commits" in git/status.rs).
#   - `issue`/`issues #N` and not a bare `#N`, which is a Rust attribute, a CSS
#     colour, or a fixture label as often as it is a tracker reference.

echo "comments-check: scanning inline comments"
echo

# Only comment LINES are scanned, so a fixture string that happens to contain
# one of these tokens is not a finding. Which marker starts a comment is
# per-language.
report=""
while IFS= read -r file; do
  [ -n "$file" ] || continue
  case "$file" in
    *.sh) prefix='^[[:space:]]*#' ;;
    *)    prefix='^[[:space:]]*(//|///|//!|/\*|\*)' ;;
  esac
  found="$(grep -nE "$prefix" "$file" 2>/dev/null | grep -E "$PATTERNS" 2>/dev/null \
             | sed "s|^|$file:|" || true)"
  [ -n "$found" ] || continue
  report="${report}${found}
"
done < <(find crates desktop-ui/src scripts -type f \
           \( -name '*.rs' -o -name '*.ts' -o -name '*.svelte' \
              -o -name '*.css' -o -name '*.sh' \) \
           -not -path '*/node_modules/*' -not -path '*/target/*' \
           -not -path '*/dist/*' \
           -not -name 'comments-check.sh' 2>/dev/null | sort)
# This script excludes itself: its own comments quote the patterns it looks for,
# and would fail it.

if [ -z "$report" ]; then
  echo "  no unresolvable citations in inline comments"
  echo
  echo "comments-check: ok"
  exit 0
fi

count=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  count=$((count + 1))
  printf '  %s\n' "$line"
done <<EOF
$report
EOF

echo
echo "comments-check: FAILED — $count comment(s) cite something a reader cannot resolve."
echo "See docs/agents/writing-docs.md, 'Inline comments'. State the constraint, or"
echo "drop the reference and put the reasoning in an ADR."
exit 1
