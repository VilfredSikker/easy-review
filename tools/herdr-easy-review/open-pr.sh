#!/usr/bin/env bash
# Link-handler action: open a clicked GitHub PR URL in Easy Review.
# Uses `er --remote <url>` so no local clone of that repo is required.
set -euo pipefail

url="${HERDR_PLUGIN_CLICKED_URL:-}"
if [[ -z "$url" ]]; then
  echo "missing HERDR_PLUGIN_CLICKED_URL" >&2
  exit 0
fi

herdr_bin="${HERDR_BIN_PATH:-herdr}"

# Open the plugin's Review pane (creates the tab if needed)…
"$herdr_bin" plugin pane open \
  --plugin easy-review \
  --entrypoint review \
  --placement tab \
  --focus || true

# …then launch er on the PR. er --remote reviews without a local clone.
if command -v er >/dev/null 2>&1; then
  exec er --remote "$url"
else
  echo "Easy Review (er) is not installed." >&2
  exit 0
fi
