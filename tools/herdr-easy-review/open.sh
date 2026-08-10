#!/usr/bin/env bash
# Pane entrypoint: open the Easy Review TUI. Herdr starts this command with
# the tab's cwd = the workspace/worktree, so `er` diffs the checked-out branch.
set -euo pipefail

if ! command -v er >/dev/null 2>&1; then
  echo "Easy Review (er) is not installed." >&2
  echo "Install it with: cargo tui-install  (or see easy-review.dev/docs)" >&2
  exit 0
fi

exec er
