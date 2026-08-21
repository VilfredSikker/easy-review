#!/usr/bin/env bash
# Verify all release version pins match (Cargo, Tauri, npm). Mirrors release.yml npm job checks.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: invalid version '$VERSION' (expected x.y.z)" >&2
  exit 1
fi

fail() {
  echo "error: $*" >&2
  exit 1
}

check_pkg_json() {
  local dir="$1"
  local pkg
  pkg="$(node -p "require('./$dir/package.json').version")"
  [[ "$pkg" == "$VERSION" ]] || fail "$dir/package.json version $pkg != $VERSION"
}

cargo_ver="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ "$cargo_ver" == "$VERSION" ]] || fail "Cargo.toml workspace version $cargo_ver != $VERSION"

tauri_ver="$(node -p "require('./crates/er-desktop/tauri.conf.json').version")"
[[ "$tauri_ver" == "$VERSION" ]] || fail "tauri.conf.json version $tauri_ver != $VERSION"

check_pkg_json "npm/er-mcp"
check_pkg_json "npm/skills"
for key in darwin-arm64 darwin-x64 linux-x64; do
  check_pkg_json "npm/platforms/$key"
done

for pkg in easy-review-mcp-darwin-arm64 easy-review-mcp-darwin-x64 easy-review-mcp-linux-x64; do
  pin="$(node -p "require('./npm/er-mcp/package.json').optionalDependencies['$pkg']")"
  [[ "$pin" == "$VERSION" ]] || fail "npm/er-mcp optionalDependencies.$pkg=$pin != $VERSION"
done

for crate in er-engine er-tui er-mcp er-desktop; do
  lock_ver="$(awk "/^name = \"$crate\"$/{getline; if (\$1 == \"version\") print \$3}" Cargo.lock | tr -d '\"')"
  [[ "$lock_ver" == "$VERSION" ]] || fail "Cargo.lock $crate version $lock_ver != $VERSION (run cargo check)"
done

echo "ok: all version pins match $VERSION"
