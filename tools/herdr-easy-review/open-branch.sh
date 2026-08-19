#!/usr/bin/env bash
# Action / event-hook: open Easy Review on the workspace's current branch.
# - As an action: targets the invoking workspace (HERDR_WORKSPACE_ID).
# - As the worktree.created event hook: targets the NEW workspace, read from
#   HERDR_PLUGIN_EVENT_JSON (which carries the opened workspace + worktree),
#   and opens WITHOUT stealing focus so the user stays on the worktree pane.
set -euo pipefail

plugin_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/json-field.sh
source "$plugin_root/lib/json-field.sh"
# shellcheck source=lib/open-review-pane.sh
source "$plugin_root/lib/open-review-pane.sh"

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

extra=()
if [[ -n "$workspace_id" ]]; then
  extra+=(--workspace "$workspace_id")
fi

open_review_pane "$focus_flag" "${extra[@]}"
exit 0
