#!/usr/bin/env bash
# Dev launcher with log groups: ./scripts/tauri-dev.sh --logs arena
# Equivalent: ER_LOG=arena cargo tauri dev  (from crates/er-desktop)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOGS=""
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --logs)
      LOGS="${2:-}"
      shift 2
      ;;
    --logs=*)
      LOGS="${1#*=}"
      shift
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done
if [[ -n "$LOGS" ]]; then
  export ER_LOG="$LOGS"
fi
export CARGO_TARGET_DIR="$ROOT/target/desktop"
"$ROOT/scripts/cargo-gc.sh" --quiet
cd "$ROOT/crates/er-desktop"
rm -rf gen/schemas
exec cargo tauri dev "${ARGS[@]}"
