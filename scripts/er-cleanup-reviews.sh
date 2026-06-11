#!/usr/bin/env bash
#
# Remove .er/review.json and related AI artifacts.
#
# Usage: scripts/er-cleanup-reviews

set -euo pipefail

rm -f .er/review.json .er/order.json .er/checklist.json .er/summary.md
echo "✓ Removed .er/review.json, order.json, checklist.json, summary.md"
