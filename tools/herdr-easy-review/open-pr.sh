#!/usr/bin/env bash
# Link-handler action: open a clicked GitHub PR URL in Easy Review.
# Opens the Review pane with HERDR_PLUGIN_CLICKED_URL so open.sh runs
# `er --remote <url>` inside the tab (actions are not pane processes).
set -euo pipefail

plugin_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/json-field.sh
source "$plugin_root/lib/json-field.sh"
# shellcheck source=lib/open-review-pane.sh
source "$plugin_root/lib/open-review-pane.sh"

url="${HERDR_PLUGIN_CLICKED_URL:-}"
if [[ -z "$url" ]]; then
  echo "missing HERDR_PLUGIN_CLICKED_URL" >&2
  exit 0
fi

# Pass the clicked URL into the pane process. Herdr does not treat
# HERDR_PLUGIN_CLICKED_URL as a managed override, so --env reaches open.sh.
open_review_pane --focus --env "HERDR_PLUGIN_CLICKED_URL=${url}"
exit 0
