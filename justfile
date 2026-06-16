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

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Short aliases
alias r := run
alias b := build
alias t := test
alias f := fmt
alias l := lint

# Default: show the grouped recipe list.
default:
    @just --list

# ─────────────────────────────── run (local / dev) ───────────────────────────────

# Run the TUI from the current git repo (dev build). Needs a real terminal.
[group('run')]
run *ARGS:
    ./scripts/er-tui.sh run -p er-tui -- {{ARGS}}

# Run the desktop app in dev mode (Tauri + Vite). `just dev --logs arena` for log groups.
[group('run')]
dev *ARGS:
    ./scripts/tauri-dev.sh {{ARGS}}

# Run only the desktop frontend (Vite dev server, no Tauri shell).
[group('run')]
dev-ui:
    cd desktop-ui && bun run dev

# Launch Storybook for the desktop UI components.
[group('run')]
storybook:
    cd desktop-ui && bun run storybook

# ─────────────────────────────────── build ───────────────────────────────────────

# Build the `er` TUI (debug).
[group('build')]
build:
    ./scripts/er-tui.sh build -p er-tui

# Build the `er` TUI (release, optimized).
[group('build')]
build-release:
    ./scripts/er-tui.sh build --release -p er-tui

# Build the desktop frontend bundle (Vite).
[group('build')]
build-ui:
    cd desktop-ui && bun run build

# Build the headless engine the way CI does (no UI features, then +highlight).
[group('build')]
build-engine-headless:
    cargo build -p er-engine --no-default-features
    cargo build -p er-engine --no-default-features --features highlight

# ──────────────────────────────── production ─────────────────────────────────────

# Install the `er` binary to ~/.cargo/bin (release). The TUI "production" artifact.
[group('production')]
install:
    ./scripts/er-tui.sh install --path crates/er-tui

# Build the production desktop bundle (.app / install). The desktop "production" artifact.
[group('production')]
build-desktop *ARGS:
    ./scripts/tauri-build.sh {{ARGS}}

# ──────────────────────────────────── test ───────────────────────────────────────

# Test the TUI + engine (scoped to target/tui — fast, no Tauri build).
[group('test')]
test:
    ./scripts/er-tui.sh test -p er-engine -p er-tui

# Test the desktop (Tauri) backend.
[group('test')]
test-desktop:
    cargo test -p er-desktop

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
