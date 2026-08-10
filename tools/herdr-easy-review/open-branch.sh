#!/usr/bin/env bash
# Action / event-hook: open Easy Review on the workspace's current branch.
# - As an action: targets the invoking workspace (HERDR_WORKSPACE_ID).
# - As the worktree.created event hook: targets the NEW workspace, read from
#   HERDR_PLUGIN_EVENT_JSON (which carries the opened workspace + worktree),
#   and opens WITHOUT stealing focus so the user stays on the worktree pane.
set -euo pipefail

# shellcheck source=lib/json-field.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib/json-field.sh"

herdr_bin="${HERDR_BIN_PATH:-herdr}"

is_event=0
if [[ -n "${HERDR_PLUGIN_EVENT:-}" ]]; then
  is_event=1
fi

workspace_id="${HERDR_WORKSPACE_ID:-}"

# worktree.created event: prefer the new workspace from the event payload.
if [[ "$is_event" -eq 1 ]]; then
  event_ws="$(json_field_string workspace_id "${HERDR_PLUGIN_EVENT_JSON:-}")"
  if [[ -n "$event_ws" ]]; then
    workspace_id="$event_ws"
  fi
fi

# Automatic opens (event hooks) run in the background; manual invocations focus.
focus_flag="--focus"
if [[ "$is_event" -eq 1 ]]; then
  focus_flag="--no-focus"
fi

args=(plugin pane open --plugin easy-review --entrypoint review --placement tab "$focus_flag")
if [[ -n "$workspace_id" ]]; then
  args+=(--workspace "$workspace_id")
fi

out="$("$herdr_bin" "${args[@]}" 2>&1 || true)"

# Rename the created tab to "Review". herdr lowercases the manifest pane title,
# so set the exact case explicitly via the returned tab_id.
tab_id="$(json_field_string tab_id "$out")"
if [[ -n "$tab_id" ]]; then
  "$herdr_bin" tab rename "$tab_id" "Review" >/dev/null 2>&1 || true
fi

exit 0
