#!/usr/bin/env bash
# Bump monorepo release version across Cargo, Tauri, and npm packages.
set -euo pipefail

usage() {
  echo "usage: $0 <new-version>   e.g. 0.4.14" >&2
  exit 1
}

[[ $# -eq 1 ]] || usage

NEW="$1"
if [[ ! "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be semver (x.y.z)" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CURRENT="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [[ -z "$CURRENT" ]]; then
  echo "error: could not read current version from Cargo.toml" >&2
  exit 1
fi

if [[ "$CURRENT" == "$NEW" ]]; then
  echo "error: already at $NEW" >&2
  exit 1
fi

echo "Bumping $CURRENT -> $NEW"

replace_in_file() {
  local file="$1"
  perl -pi -e "s/\\Q$CURRENT\\E/$NEW/g" "$file"
}

replace_in_file "Cargo.toml"
replace_in_file "crates/er-desktop/tauri.conf.json"
replace_in_file "npm/er-mcp/package.json"
replace_in_file "npm/platforms/darwin-arm64/package.json"
replace_in_file "npm/platforms/darwin-x64/package.json"
replace_in_file "npm/platforms/linux-x64/package.json"
replace_in_file "npm/skills/package.json"
replace_in_file "npm/skills/package-lock.json"

echo "Updated version files. Next:"
echo "  ./scripts/er-tui.sh check -p er-engine -p er-tui -p er-mcp"
echo "  CARGO_TARGET_DIR=target/desktop cargo check -p er-desktop"
echo "  ./scripts/verify-release-versions.sh $NEW"
