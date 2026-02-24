#!/usr/bin/env bash
#
# Generate sample .er-* files in the current repo for testing the AI overlay.
#
# Usage:
#   cd your-repo
#   bash /path/to/generate-test-fixtures.sh
#
# This reads the current git diff (branch mode, unstaged, or staged) and
# creates .er-review.json, .er-order.json, .er-checklist.json, and .er-summary.md
# with realistic but synthetic data. The diff_hash matches the current diff
# so the data will NOT show as stale.
#
# To test stale detection, edit a file after running this script.

set -euo pipefail

# Determine which diff to hash (branch mode = current branch vs main/master)
BASE=$(git rev-parse --verify main 2>/dev/null || git rev-parse --verify master 2>/dev/null || echo "HEAD~1")
RAW_DIFF=$(git diff --no-ext-diff --no-color "$BASE"...HEAD 2>/dev/null || git diff --no-ext-diff --no-color HEAD 2>/dev/null || echo "")

if [ -z "$RAW_DIFF" ]; then
    echo "No diff found. Trying unstaged changes..."
    RAW_DIFF=$(git diff --no-ext-diff --no-color 2>/dev/null || echo "")
fi

if [ -z "$RAW_DIFF" ]; then
    echo "No changes detected. Creating fixtures with dummy hash."
    DIFF_HASH="0000000000000000000000000000000000000000000000000000000000000000"
    FILES='[]'
else
    DIFF_HASH=$(echo -n "$RAW_DIFF" | shasum -a 256 | cut -d' ' -f1)
    # Extract changed file paths
    FILES=$(echo "$RAW_DIFF" | grep -E '^\+\+\+ b/' | sed 's|^+++ b/||' | head -20)
fi

echo "Diff hash: $DIFF_HASH"
echo "Changed files:"
echo "$FILES"
echo ""

# Build .er-review.json from the actual changed files
REVIEW='{"version":1,"diff_hash":"'"$DIFF_HASH"'","created_at":"'"$(date -u +%Y-%m-%dT%H:%M:%SZ)"'","files":{'

FIRST=true
HUNK_IDX=0
FINDING_ID=1
RISKS=("high" "medium" "low" "info")
CATEGORIES=("security" "logic" "performance" "style" "correctness" "error-handling")
while IFS= read -r filepath; do
    [ -z "$filepath" ] && continue

    # Pick a risk level based on hash of filename
    HASH_VAL=$(echo -n "$filepath" | cksum | cut -d' ' -f1)
    RISK_IDX=$((HASH_VAL % 4))
    RISK="${RISKS[$RISK_IDX]}"

    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        REVIEW+=','
    fi

    REASON="Changes to $(basename "$filepath") affect core logic"
    SUMMARY="Modifies $(basename "$filepath") — review for side effects"

    # Generate 1-2 findings per file
    CAT_IDX=$((HASH_VAL % 6))
    CAT="${CATEGORIES[$CAT_IDX]}"

    FINDING_SEV="${RISKS[$((HASH_VAL % 3))]}"

    REVIEW+='"'"$filepath"'":{"risk":"'"$RISK"'","risk_reason":"'"$REASON"'","summary":"'"$SUMMARY"'","findings":['
    REVIEW+='{"id":"f-'"$FINDING_ID"'","severity":"'"$FINDING_SEV"'","category":"'"$CAT"'","title":"Check error handling in this change","description":"The modified code path does not handle the error case when the input is None/null. Consider adding a guard clause.","hunk_index":0,"line_start":null,"line_end":null,"suggestion":"Add an early return or match on the Option/Result type.","related_files":[],"responses":[]}'
    FINDING_ID=$((FINDING_ID + 1))

    # Second finding for high-risk files
    if [ "$RISK" = "high" ] || [ "$RISK" = "medium" ]; then
        CAT2_IDX=$(((HASH_VAL + 1) % 6))
        CAT2="${CATEGORIES[$CAT2_IDX]}"
        REVIEW+=',{"id":"f-'"$FINDING_ID"'","severity":"low","category":"'"$CAT2"'","title":"Consider adding a test for this path","description":"This change introduces a new branch that is not covered by existing tests.","hunk_index":0,"line_start":null,"line_end":null,"suggestion":"Add a unit test covering the new conditional.","related_files":[],"responses":[]}'
        FINDING_ID=$((FINDING_ID + 1))
    fi

    REVIEW+=']}'

    HUNK_IDX=$((HUNK_IDX + 1))
done <<< "$FILES"

REVIEW+='}}'

echo "$REVIEW" | python3 -m json.tool > .er-review.json 2>/dev/null || echo "$REVIEW" > .er-review.json
echo "✓ Created .er-review.json"

# Build .er-order.json
ORDER='{"version":1,"diff_hash":"'"$DIFF_HASH"'","order":['
FIRST=true
while IFS= read -r filepath; do
    [ -z "$filepath" ] && continue
    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        ORDER+=','
    fi
    ORDER+='{"path":"'"$filepath"'","reason":"Core change","group":"main"}'
done <<< "$FILES"
ORDER+='],"groups":{"main":{"label":"Main Changes","color":"blue"}}}'

echo "$ORDER" | python3 -m json.tool > .er-order.json 2>/dev/null || echo "$ORDER" > .er-order.json
echo "✓ Created .er-order.json"

# Build .er-checklist.json
FILE_COUNT=$(echo "$FILES" | grep -c '.' 2>/dev/null || true)
CHECKLIST='{"version":1,"diff_hash":"'"$DIFF_HASH"'","items":['
CHECKLIST+='{"id":"c-1","text":"Verify all error paths are handled","category":"correctness","checked":false,"related_findings":["f-1"],"related_files":[]},'
CHECKLIST+='{"id":"c-2","text":"Check for breaking API changes","category":"compatibility","checked":false,"related_findings":[],"related_files":[]},'
CHECKLIST+='{"id":"c-3","text":"Review test coverage for new code paths","category":"testing","checked":false,"related_findings":[],"related_files":[]},'
CHECKLIST+='{"id":"c-4","text":"Confirm no secrets or credentials in diff","category":"security","checked":true,"related_findings":[],"related_files":[]}'
CHECKLIST+=']}'

echo "$CHECKLIST" | python3 -m json.tool > .er-checklist.json 2>/dev/null || echo "$CHECKLIST" > .er-checklist.json
echo "✓ Created .er-checklist.json"

# Build .er-summary.md
cat > .er-summary.md << 'SUMMARY'
## AI Review Summary

This changeset modifies core application logic across multiple files.

**Key observations:**
- Error handling paths should be reviewed carefully
- Some new code branches lack test coverage
- No credential or secret exposure detected

**Risk assessment:** Medium overall — most changes are straightforward but
the error handling gaps should be addressed before merge.
SUMMARY
echo "✓ Created .er-summary.md"

echo ""
echo "Done! Open 'er' and press 'v' to toggle the AI overlay."
echo "To test stale detection: edit any tracked file, then reopen er."
echo ""
echo "To clean up: rm .er-review.json .er-order.json .er-checklist.json .er-summary.md"
