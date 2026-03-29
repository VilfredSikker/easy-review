#!/usr/bin/env bash
# Airlock config for easy-review (Rust TUI)
#
# Note: Rust tools (cargo fmt, clippy, cargo test) are project-wide commands
# that don't accept individual file paths. Commands end with `#` so that
# any file arguments appended by airlock are treated as a shell comment.

# --- Format ---
FORMAT_CMD="cargo fmt --all -- --check #"
FORMAT_FIX_CMD="cargo fmt --all #"
FORMAT_EXTENSIONS="rs"

# --- Lint ---
LINT_CMD="cargo clippy --all-targets -- -D warnings #"
LINT_FIX_CMD="cargo clippy --all-targets --fix --allow-dirty -- -D warnings #"
LINT_EXTENSIONS="rs"

# --- Tests (Tier 2) ---
TEST_CMD="cargo test #"

# --- Type check (Tier 3) ---
TYPECHECK_CMD="cargo check --all-targets #"

# --- Ignore patterns ---
IGNORE_PATTERNS="target/|.git/|.er/|.work/"

# --- Base branch ---
BASE_BRANCH="main"
