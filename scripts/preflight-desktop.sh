#!/usr/bin/env bash
# Shared preflight for the desktop dev/build scripts.
# Verifies the toolchain a fresh clone needs (tauri-cli + bun) and installs the
# frontend deps on first run, so a missing prerequisite prints an actionable fix
# up front instead of failing cryptically inside `cargo tauri` / Vite later.
# Sourced by tauri-dev.sh and tauri-build.sh; returns non-zero on the first
# missing prerequisite (callers run under `set -e`, so that aborts the script).

preflight_desktop() {
  local root="$1"

  if ! command -v cargo >/dev/null 2>&1; then
    cat >&2 <<'EOF'
error: cargo (Rust toolchain) is not installed (or not on PATH).

The desktop app is built with Rust. Install it once:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

open a new shell so cargo is on PATH, then re-run this script.
EOF
    return 1
  fi

  if ! cargo tauri --version >/dev/null 2>&1; then
    cat >&2 <<'EOF'
error: the Tauri CLI (cargo-tauri) is not installed.

The desktop app builds with Tauri, which provides the `cargo tauri` subcommand.
Install it once:

    cargo install tauri-cli --locked

then re-run this script.
EOF
    return 1
  fi

  if ! command -v bun >/dev/null 2>&1; then
    cat >&2 <<'EOF'
error: bun is not installed (or not on PATH).

The desktop frontend (desktop-ui) builds with bun. Install it once:

    curl -fsSL https://bun.sh/install | bash      # or: brew install oven-sh/bun/bun

open a new shell so bun is on PATH, then re-run this script.
EOF
    return 1
  fi

  # Frontend deps aren't vendored, and Tauri's beforeDev/beforeBuild command runs
  # `bun run dev` / `bun run build`, which needs node_modules present. Install on
  # first run so a fresh clone doesn't hit an opaque Vite "cannot find module".
  if [[ ! -d "$root/desktop-ui/node_modules" ]]; then
    echo "preflight: installing desktop-ui dependencies (bun install)…" >&2
    (cd "$root/desktop-ui" && bun install)
  fi
}
