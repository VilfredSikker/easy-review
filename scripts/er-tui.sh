#!/usr/bin/env bash
# TUI-scoped cargo wrapper — keeps desktop/Tauri artifacts out of the default target dir.
# Usage: ./scripts/er-tui.sh build | test | run -p er-tui -- [args]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export CARGO_TARGET_DIR="$ROOT/target/tui"
"$ROOT/scripts/cargo-gc.sh" --quiet
exec cargo "$@"
