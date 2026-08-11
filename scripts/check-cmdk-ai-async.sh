#!/usr/bin/env bash
# Regression guard for ⌘K / AI Hub action freezes.
#
# Sync Tauri commands run on the main thread. Palette actions that lock App and
# rebuild a snapshot (run review, change model, …) must be `pub async fn` whose
# body uses `run_blocking` (or is a thin `.await` wrapper around another such
# command) — same pattern as the file-filter freeze fix.
#
# Exit 0 = green. Exit 1 = red.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/crates/er-desktop/src/commands.rs"

# Leaf commands: must be async AND contain run_blocking in their body.
MUST_RUN_BLOCKING=(
  set_ai_selection
  set_ai_model
  set_ai_effort
  list_ai_providers
  run_ai_review
  run_ai_scoped_review
  run_ai_validate
  run_ai_expert_review
  generate_tour
  export_to_agent
  refresh_diff
  force_refresh_diff
)

# Thin wrappers that only .await another async command (no own run_blocking).
MUST_BE_ASYNC_WRAPPER=(
  run_ai_triage_review
  run_ai_review_files
  run_ai_professor_review
)

# Extract the source slice for `pub async fn NAME` up to the next top-level
# `pub (async )?fn` / `#[tauri::command]` / end of file.
function_body() {
  local name="$1"
  python3 - "$SRC" "$name" <<'PY'
import re, sys
src, name = open(sys.argv[1]).read(), sys.argv[2]
m = re.search(rf"(?m)^pub async fn {re.escape(name)}\b", src)
if not m:
    sys.exit(2)
start = m.start()
rest = src[start:]
# Next top-level item after this fn's signature line.
nxt = re.search(r"(?m)^(?:#\[tauri::command\]|pub (?:async )?fn |pub use )", rest[1:])
end = (1 + nxt.start()) if nxt else len(rest)
print(rest[:end])
PY
}

failures=0

for name in "${MUST_RUN_BLOCKING[@]}"; do
  if ! grep -qE "^pub async fn ${name}\\b" "$SRC"; then
    if grep -qE "^pub fn ${name}\\b" "$SRC"; then
      echo "RED: ${name} is still \`pub fn\` (sync / main-thread) — must be \`pub async fn\` + run_blocking"
    else
      echo "RED: ${name} signature not found in commands.rs"
    fi
    failures=$((failures + 1))
    continue
  fi
  body="$(function_body "$name" || true)"
  if [[ -z "$body" ]]; then
    echo "RED: ${name} body could not be extracted"
    failures=$((failures + 1))
    continue
  fi
  if ! grep -q "run_blocking" <<<"$body"; then
    echo "RED: ${name} is async but its body has no run_blocking"
    failures=$((failures + 1))
  fi
done

for name in "${MUST_BE_ASYNC_WRAPPER[@]}"; do
  if ! grep -qE "^pub async fn ${name}\\b" "$SRC"; then
    if grep -qE "^pub fn ${name}\\b" "$SRC"; then
      echo "RED: ${name} is still \`pub fn\` — must be \`pub async fn\` wrapper"
    else
      echo "RED: ${name} signature not found in commands.rs"
    fi
    failures=$((failures + 1))
    continue
  fi
  body="$(function_body "$name" || true)"
  if [[ -z "$body" ]]; then
    echo "RED: ${name} body could not be extracted"
    failures=$((failures + 1))
    continue
  fi
  if ! grep -q "\.await" <<<"$body"; then
    echo "RED: ${name} wrapper body has no .await"
    failures=$((failures + 1))
  fi
  if grep -q "state\.app\.lock" <<<"$body"; then
    echo "RED: ${name} should stay a thin .await wrapper (no App lock)"
    failures=$((failures + 1))
  fi
done

if [[ "$failures" -gt 0 ]]; then
  echo "FAIL: ${failures} ⌘K/AI action(s) would freeze the UI on the main thread"
  exit 1
fi

echo "OK: all ⌘K/AI palette-hot commands are async + run_blocking (or thin wrappers)"
exit 0
