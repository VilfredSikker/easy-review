#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
  cat >&2 <<'EOF'
error: cargo (Rust toolchain) is not installed or not on PATH.

Install Rust with rustup, then open a new shell and retry:

    https://rustup.rs/
EOF
  exit 1
fi
if ! command -v rustc >/dev/null 2>&1; then
  echo "error: rustc is not on PATH. Activate or install the Rust toolchain with rustup, then retry." >&2
  exit 1
fi

rust_version="$(rustc --version | awk '{print $2}')"
IFS=. read -r rust_major rust_minor _ <<<"$rust_version"
if [[ ! "$rust_major" =~ ^[0-9]+$ || ! "$rust_minor" =~ ^[0-9]+$ ]] \
  || (( rust_major < 1 || (rust_major == 1 && rust_minor < 88) )); then
  cat >&2 <<EOF
error: Rust 1.88 or newer is required; found ${rust_version:-unknown}.

Update the active toolchain with rustup:

    rustup update stable
EOF
  exit 1
fi

if ! command -v bun >/dev/null 2>&1; then
  cat >&2 <<'EOF'
error: bun is not installed or not on PATH.

Install Bun, then open a new shell and retry:

    https://bun.sh/docs/installation
EOF
  exit 1
fi

if ! tauri_version="$(cargo tauri --version 2>/dev/null)" \
  || [[ "$tauri_version" != "tauri-cli 2."* ]]; then
  echo "install-deps: installing Tauri CLI 2 (cargo-tauri)…" >&2
  cargo install tauri-cli --version '^2' --locked
fi

echo "install-deps: fetching locked Rust workspace dependencies…" >&2
(cd "$root" && cargo fetch --locked)

node_modules="$root/desktop-ui/node_modules"
if [[ -L "$node_modules" ]]; then
  echo "install-deps: node_modules is a symlink; Bun will update its shared target at $(readlink "$node_modules")." >&2
fi
echo "install-deps: installing desktop-ui dependencies from bun.lock…" >&2
(cd "$root/desktop-ui" && bun install --frozen-lockfile)

echo "install-deps: done" >&2
