# Shared JSON helpers for Herdr plugin shell entrypoints.
# Herdr event/context payloads are small single-line JSON blobs; these helpers
# avoid a jq/python dependency for the handful of string fields we read.

json_field_string() {
  local key="$1"
  printf '%s' "${2:-}" | sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" | head -n1
}
