#!/usr/bin/env bash
# Pane entrypoint: open the Easy Review TUI.
# Herdr starts plugin pane commands in the plugin directory so the relative
# `bash open.sh` command resolves. That is not the workspace checkout.
# Without an explicit cd, `er` reviews the plugin's own git repo.
# When opened for a PR link (via open-pr.sh --env), review that URL remotely.
set -euo pipefail

plugin_root="${HERDR_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
# shellcheck source=lib/json-field.sh
source "$plugin_root/lib/json-field.sh"
# shellcheck source=lib/review-cwd.sh
source "$plugin_root/lib/review-cwd.sh"

if ! command -v er >/dev/null 2>&1; then
  echo "Easy Review (er) is not installed." >&2
  echo "Install it with: cargo tui-install  (or see easy-review.dev/docs)" >&2
  exit 0
fi

review_dir="$(review_cwd)"
if [[ -n "$review_dir" && -d "$review_dir" ]]; then
  cd -- "$review_dir"
fi

url="${HERDR_PLUGIN_CLICKED_URL:-}"
if [[ -n "$url" ]]; then
  exec er --remote "$url"
fi

exec er
