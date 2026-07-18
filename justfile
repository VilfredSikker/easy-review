# easy-review (`er`) — task runner. https://just.systems
#
# `just`            list all recipes (grouped)
# `just <recipe>`   run one
#
# This file is a thin front-end over the existing wrapper scripts and cargo
# aliases — it does not reinvent them. The scripts are the single source of
# truth for the split target dirs (`target/tui` vs `target/desktop`, so a
# desktop/Tauri build never bloats the TUI's incremental cache) and the
# auto cargo-gc. See AGENTS.md "Build / Test / Lint / Run" and CLAUDE.md.
#
# Env passthrough still works, e.g.  ER_DEBUG=1 just run   |   just dev --logs arena
# Pass extra args after the recipe name where it takes `*ARGS`.
#
# Recipes are grouped DEVELOPMENT (dev/tui, dev/desktop) and
# PRODUCTION (prod/tui, prod/desktop). For shipping the desktop app:
#   just build-desktop           bundle the release .app only (ad-hoc)
#   just install-desktop         bundle + copy to /Applications
#   just release-desktop         bundle + install + DMG (ad-hoc)
#   just sign-release-desktop    Developer ID signed + notarized .app/.dmg

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Short aliases
alias r := run
alias b := build
alias t := test
alias f := fmt
alias l := lint
alias sign := sign-release-desktop

# Bare `just` lists every recipe. The `_` prefix hides this entry from the list itself.
_default:
    @just --list

# ════════════════════════════════ DEVELOPMENT ════════════════════════════════════
# Local dev builds and dev servers. Fast, unoptimized, never installs anything.

# ───────────────────────────────────── TUI ───────────────────────────────────────

# Run the TUI from the current git repo (dev build). Needs a real terminal.
[group('dev/tui')]
run *ARGS:
    ./scripts/er-tui.sh run -p er-tui -- {{ARGS}}

# Build the `er` TUI (debug, unoptimized).
[group('dev/tui')]
build:
    ./scripts/er-tui.sh build -p er-tui

# ─────────────────────────────────── DESKTOP ─────────────────────────────────────

# Run the desktop app in dev mode (Tauri + Vite). `just dev --logs arena` for log groups.
[group('dev/desktop')]
dev *ARGS:
    ./scripts/tauri-dev.sh {{ARGS}}

# Run only the desktop frontend (Vite dev server, no Tauri shell).
[group('dev/desktop')]
dev-ui:
    cd desktop-ui && bun run dev

# Build just the desktop frontend bundle (Vite) — does NOT build the Tauri shell.
[group('dev/desktop')]
build-ui:
    cd desktop-ui && bun run build

# Launch Storybook for the desktop UI components.
[group('dev/desktop')]
storybook:
    cd desktop-ui && bun run storybook

# ══════════════════════════ PRODUCTION · RELEASE · INSTALL ════════════════════════
# Optimized, shippable artifacts. The desktop trio is build → install → release.

# ───────────────────────────────────── TUI ───────────────────────────────────────

# Build the `er` TUI binary (release, optimized) — leaves it in target/tui, no install.
[group('prod/tui')]
build-release:
    ./scripts/er-tui.sh build --release -p er-tui

# Build + install the `er` binary to ~/.cargo/bin (release). The TUI production install.
[group('prod/tui')]
install:
    ./scripts/er-tui.sh install --path crates/er-tui

# ─────────────────────────────────── DESKTOP ─────────────────────────────────────
# All three shell out to scripts/tauri-build.sh (release .app in target/desktop).
# They differ only in what happens after the bundle is built:
#   build-desktop   → bundle only             (ER_SKIP_INSTALL=1)
#   install-desktop → bundle + copy to /Applications
#   release-desktop → bundle + install + DMG  (ER_SKIP_DMG=0)

# Build the release desktop .app bundle only — does NOT install or make a DMG.
[group('prod/desktop')]
build-desktop *ARGS:
    ER_SKIP_INSTALL=1 ./scripts/tauri-build.sh {{ARGS}}

# Build + install the desktop app to /Applications (ad-hoc signed, quarantine cleared).
[group('prod/desktop')]
install-desktop *ARGS:
    ./scripts/tauri-build.sh {{ARGS}}

# Build + install + produce a distributable DMG under target/desktop/release/bundle/dmg.
[group('prod/desktop')]
release-desktop *ARGS:
    ER_SKIP_DMG=0 ./scripts/tauri-build.sh {{ARGS}}

# Developer ID signed + notarized .app/.dmg (needs `.env.signing`).
[group('prod/desktop')]
sign-release-desktop *ARGS:
    ./scripts/tauri-sign-release.sh {{ARGS}}

# ──────────────────────────────────── test ───────────────────────────────────────

# Test the TUI + engine (scoped to target/tui — fast, no Tauri build).
[group('test')]
test:
    ./scripts/er-tui.sh test -p er-engine -p er-tui

# Test the desktop (Tauri) backend.
[group('test')]
test-desktop:
    cargo test -p er-desktop

# Build the Easy Review MCP server (stdio).
[group('dev/mcp')]
build-mcp:
    cargo build -p er-mcp

# Install `er-mcp` to ~/.cargo/bin.
[group('prod/mcp')]
install-mcp:
    cargo install --path crates/er-mcp

# Test the npm launcher (Node 18+).
[group('test')]
test-mcp-npm:
    cd npm/er-mcp && npm test

# Test the desktop frontend (bun).
[group('test')]
test-ui:
    cd desktop-ui && bun test src

# Test the entire workspace (slow — compiles Tauri into target/).
[group('test')]
test-all:
    cargo test --workspace

# ─────────────────────────────── format & lint ───────────────────────────────────

# Format all Rust code.
[group('lint')]
fmt:
    cargo fmt --all

# Check formatting without writing (CI gate).
[group('lint')]
fmt-check:
    cargo fmt --all -- --check

# Clippy across the whole workspace, warnings = errors.
[group('lint')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Type-check the workspace without producing binaries.
[group('lint')]
check:
    cargo check --all-targets

# Type-check the desktop frontend (svelte-check).
[group('lint')]
check-ui:
    cd desktop-ui && bun run check

# All static checks: rustfmt, clippy, and the frontend type-check.
[group('lint')]
lint: fmt-check clippy check-ui

# ───────────────────────────────── aggregates ────────────────────────────────────

# Mirror the GitHub CI gate: format, clippy, tests, headless engine builds.
[group('ci')]
ci: fmt-check clippy test build-engine-headless

# Build the headless engine the way CI does (no UI features, then +highlight).
[group('ci')]
build-engine-headless:
    cargo build -p er-engine --no-default-features
    cargo build -p er-engine --no-default-features --features highlight

# Fast local pre-commit gate: format the tree, clippy, then TUI/engine tests.
[group('ci')]
verify: fmt clippy test

# ──────────────────────────────── maintenance ────────────────────────────────────

# Reclaim disk from bloated cargo target dirs. `just gc --force` also prunes target/desktop.
[group('maint')]
gc *ARGS:
    ./scripts/cargo-gc.sh {{ARGS}}

# Generate sample .er/ AI-review fixtures in the current repo (for testing the overlay).
[group('maint')]
fixtures:
    bash scripts/generate-test-fixtures.sh

# Remove all cargo build artifacts (target/, target/tui, target/desktop).
[group('maint')]
clean:
    cargo clean
    rm -rf target/tui target/desktop
