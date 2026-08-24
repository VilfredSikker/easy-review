#!/usr/bin/env bash
# Report how long since the last `just mutants` run; exit 1 when it is stale
# (never run, or older than the threshold — default 30 days).
#
# Mutation testing is deliberately NOT scheduled: it is expensive, so we run
# it on demand and let this check tell us when the log has gone stale.
set -eu

LOG="$(cd "$(dirname "$0")/.." && pwd)/quality/mutants-last-run.txt"
DAYS="${1:-30}"

if [ ! -f "$LOG" ]; then
  echo 'Mutation testing has never run (no quality/mutants-last-run.txt).'
  echo 'Run `just mutants` to get a baseline.'
  exit 1
fi

LAST="$(cat "$LOG")"
if [ "$LAST" = "never" ]; then
  echo 'Mutation testing has never run. Run `just mutants` to get a baseline.'
  exit 1
fi

# macOS: date -j -f; Linux: date -d.
LAST_EPOCH="$(date -j -f %F "$LAST" +%s 2>/dev/null || date -d "$LAST" +%s 2>/dev/null || echo 0)"
AGE=$(( ($(date +%s) - LAST_EPOCH) / 86400 ))
echo "Last mutation run: $LAST ($AGE day(s) ago)"

if [ "$AGE" -gt "$DAYS" ]; then
  echo "Stale — older than $DAYS day(s). Consider \`just mutants\`."
  exit 1
fi
echo "Fresh enough (within $DAYS day(s))."
