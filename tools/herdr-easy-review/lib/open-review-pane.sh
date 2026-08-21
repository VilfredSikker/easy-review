# Shared Review-pane opener for Herdr plugin actions / event hooks.
# Requires: lib/json-field.sh already sourced.
#
# Usage: open_review_pane [--focus|--no-focus] [--workspace ID] [--env KEY=VAL]...
# Opens the review entrypoint as a tab, then renames it to "Review"
# (Herdr lowercases the manifest pane title).
#
# Do not pass --cwd. Herdr would start the pane command there, and the
# manifest's relative `bash open.sh` would fail to find the script.
# open.sh cds to the workspace checkout after it starts.
open_review_pane() {
  local herdr_bin="${HERDR_BIN_PATH:-herdr}"
  local args=(plugin pane open --plugin easy-review --entrypoint review --placement tab)
  args+=("$@")

  local out
  out="$("$herdr_bin" "${args[@]}" 2>&1 || true)"

  local tab_id
  tab_id="$(json_field_string tab_id "$out")"
  if [[ -n "$tab_id" ]]; then
    "$herdr_bin" tab rename "$tab_id" "Review" >/dev/null 2>&1 || true
  fi
}
