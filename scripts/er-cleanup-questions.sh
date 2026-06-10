#!/usr/bin/env bash
#
# Remove .er/questions.json (and its archive).
#
# Usage: scripts/er-cleanup-questions

set -euo pipefail

rm -f .er/questions.json .er/questions.prev.json
echo "✓ Removed .er/questions.json"
