#!/usr/bin/env bash
# Regression guard for ⌘K / AI Hub action freezes.
#
# Sync Tauri commands run on the main thread. Palette actions that lock App and
# rebuild a snapshot (run review, change model, …) must be `pub async fn` +
# `run_blocking` — same pattern as the file-filter freeze fix.
#
# Exit 0 = green (all cmds async). Exit 1 = red (at least one still sync).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/crates/er-desktop/src/commands.rs"

MUST_BE_ASYNC=(
  set_ai_selection
  set_ai_model
  set_ai_effort
  list_ai_providers
  run_ai_review
  run_ai_triage_review
  run_ai_scoped_review
  run_ai_review_files
  run_ai_validate
  run_ai_expert_review
  run_ai_professor_review
  generate_tour
  export_to_agent
  refresh_diff
)

failures=0
for name in "${MUST_BE_ASYNC[@]}"; do
  if grep -qE "^pub async fn ${name}\\b" "$SRC"; then
    continue
  fi
  if grep -qE "^pub fn ${name}\\b" "$SRC"; then
    echo "RED: ${name} is still \`pub fn\` (sync / main-thread) — must be \`pub async fn\` + run_blocking"
    failures=$((failures + 1))
  else
    echo "RED: ${name} signature not found in commands.rs"
    failures=$((failures + 1))
  fi
done

if [[ "$failures" -gt 0 ]]; then
  echo "FAIL: ${failures} ⌘K/AI action(s) would freeze the UI on the main thread"
  exit 1
fi

echo "OK: all ⌘K/AI palette-hot commands are async"
exit 0
