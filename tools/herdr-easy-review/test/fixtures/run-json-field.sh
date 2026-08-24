#!/usr/bin/env bash
# Test fixture: source json-field.sh and print extracted fields.
set -euo pipefail
# shellcheck source=../../lib/json-field.sh
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/lib/json-field.sh"
printf '%s|%s\n' \
  "$(json_field_string workspace_id "${JSON_BLOB:-}")" \
  "$(json_field_string tab_id "${JSON_BLOB:-}")"
