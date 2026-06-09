#!/usr/bin/env bash
# Reclaim disk from bloated Cargo artifact dirs (orphaned debug/deps, legacy layout).
# Called automatically from dev scripts; run manually: ./scripts/cargo-gc.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
QUIET=0
FORCE=0

usage() {
  echo "Usage: cargo-gc.sh [--quiet] [--force]"
  echo "  Prunes legacy target/debug + target/release when over size/file thresholds."
  echo "  With --force, also prunes target/desktop (next Tauri dev rebuilds it)."
  echo "  Env: ER_CARGO_GC_MAX_GB (default 8), ER_CARGO_GC_MAX_FILES (default 80000)"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --quiet) QUIET=1; shift ;;
    --force) FORCE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1"; usage; exit 1 ;;
  esac
done

log() {
  if [[ "$QUIET" -eq 0 ]]; then
    echo "$@"
  fi
}

dir_size_kb() {
  du -sk "$1" 2>/dev/null | cut -f1 || echo 0
}

file_count() {
  find "$1" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' '
}

should_prune_dir() {
  local dir="$1"
  [[ -d "$dir" ]] || return 1
  local max_gb="${ER_CARGO_GC_MAX_GB:-8}"
  local max_files="${ER_CARGO_GC_MAX_FILES:-80000}"
  local kb files
  kb=$(dir_size_kb "$dir")
  files=$(file_count "$dir")
  (( kb > max_gb * 1024 * 1024 || files > max_files ))
}

prune_dir() {
  local dir="$1"
  local label="$2"
  if should_prune_dir "$dir"; then
    log "er cargo-gc: pruning $label ($(du -sh "$dir" 2>/dev/null | cut -f1), $(file_count "$dir") files)"
    rm -rf "$dir"
    return 0
  fi
  return 1
}

pruned=0

# Legacy monolithic target/ (before target/tui + target/desktop split).
for legacy in debug release incremental build; do
  if prune_dir "$ROOT/target/$legacy" "target/$legacy"; then
    pruned=1
  fi
done

if [[ "$FORCE" -eq 1 ]] && [[ -d "$ROOT/target/desktop" ]]; then
  log "er cargo-gc: pruning target/desktop (--force)"
  rm -rf "$ROOT/target/desktop"
  pruned=1
fi

if [[ "$pruned" -eq 0 && "$QUIET" -eq 0 ]]; then
  for d in "$ROOT/target/tui" "$ROOT/target/desktop" "$ROOT/target"; do
    if [[ -d "$d" ]]; then
      log "  $(du -sh "$d" 2>/dev/null | cut -f1)  $d"
    fi
  done
fi
