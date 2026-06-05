#!/usr/bin/env bash
# Cloud-agent VM bootstrap for easy-review.
# Only runs `rustup update` when stable is older than MIN_RUST_VERSION.
# Unconditional updates fail on overlayfs when RUSTUP_HOME=/usr/local/rustup (EXDEV on rename).
set -euo pipefail

MIN_RUST_VERSION="${MIN_RUST_VERSION:-1.85.0}"

printf '>>> [install:] start\n'

rust_version_at_least() {
  local min="$1"
  local ver major minor patch min_major min_minor min_patch
  ver=$(rustc --version 2>/dev/null | awk '{print $2}' | cut -d- -f1) || return 1
  IFS=. read -r major minor patch <<<"$ver"
  IFS=. read -r min_major min_minor min_patch <<<"$min"
  if (( major > min_major )); then return 0; fi
  if (( major < min_major )); then return 1; fi
  if (( minor > min_minor )); then return 0; fi
  if (( minor < min_minor )); then return 1; fi
  return 0
}

if rust_version_at_least "$MIN_RUST_VERSION"; then
  echo "Rust >= ${MIN_RUST_VERSION} already installed ($(rustc --version)), skipping rustup update"
else
  echo "Rust < ${MIN_RUST_VERSION} or missing; running rustup update stable"
  rustup update stable
fi

rustup default stable

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
cargo fetch

printf '<<< [install:] complete\n'
