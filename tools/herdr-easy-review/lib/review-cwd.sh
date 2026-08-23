# Resolve the git checkout Herdr wants `er` to review.
# Plugin pane commands start in the plugin directory (HERDR_PLUGIN_ROOT),
# not the workspace, so callers must cd here before exec'ing er.
#
# Preference:
#   1. worktree.checkout_path (workspace identity; stable if a Review tab is focused)
#   2. workspace_cwd
#   3. focused_pane_cwd
review_cwd() {
  local json="${1:-${HERDR_PLUGIN_CONTEXT_JSON:-}}"
  local cwd=""
  cwd="$(json_field_string checkout_path "$json")"
  if [[ -z "$cwd" ]]; then
    cwd="$(json_field_string workspace_cwd "$json")"
  fi
  if [[ -z "$cwd" ]]; then
    cwd="$(json_field_string focused_pane_cwd "$json")"
  fi
  printf '%s' "$cwd"
}
