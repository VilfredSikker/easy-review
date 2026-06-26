#!/usr/bin/env bash
# Dev launcher with log groups: ./scripts/tauri-dev.sh --logs arena
# Equivalent: ER_LOG=arena cargo tauri dev  (from crates/er-desktop)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOGS=""
ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --logs)
      LOGS="${2:-}"
      shift 2
      ;;
    --logs=*)
      LOGS="${1#*=}"
      shift
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done
if [[ -n "$LOGS" ]]; then
  export ER_LOG="$LOGS"
fi

# Pick a free dev-server port (start at 5183, the default), so a stale server
# holding 5183 doesn't block launch. Vite (VITE_DEV_PORT) and Tauri (devUrl)
# are both pointed at the same chosen port so the webview loads the right server.
find_free_port() {
  local port="$1"
  local max=$((port + 20))
  while [[ "$port" -lt "$max" ]]; do
    if ! lsof -iTCP:"$port" -sTCP:LISTEN -n -P >/dev/null 2>&1; then
      echo "$port"
      return 0
    fi
    port=$((port + 1))
  done
  echo "$1" # all busy: fall back to the base; Vite's strictPort will fail loudly
}
DEV_PORT="$(find_free_port "${ER_DEV_PORT:-5183}")"
export VITE_DEV_PORT="$DEV_PORT"
if [[ "$DEV_PORT" != "${ER_DEV_PORT:-5183}" ]]; then
  echo "tauri-dev: port ${ER_DEV_PORT:-5183} busy, using $DEV_PORT" >&2
fi

export CARGO_TARGET_DIR="$ROOT/target/desktop"
"$ROOT/scripts/cargo-gc.sh" --quiet
cd "$ROOT/crates/er-desktop"
rm -rf gen/schemas
exec cargo tauri dev \
  --config "{\"build\":{\"devUrl\":\"http://localhost:$DEV_PORT\"}}" \
  ${ARGS[@]+"${ARGS[@]}"}
