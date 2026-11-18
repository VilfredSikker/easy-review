#!/usr/bin/env bash
# Fail when the committed TypeScript for the wire types differs from a fresh
# export. The export goes into a temporary directory, so the check never writes
# the tree, and `diff -r` also reports a file present on one side only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GENERATED=desktop-ui/src/lib/protocol/generated
fresh="$(mktemp -d)"
trap 'rm -rf "$fresh"' EXIT

cargo run -q -p er-protocol --bin protocol-gen -- "$fresh"

if ! diff -ru "$GENERATED" "$fresh"; then
  echo >&2
  echo "protocol-check: $GENERATED differs from the wire types." >&2
  echo "Run \`just protocol-gen\` and commit the result; never edit these files by hand." >&2
  exit 1
fi
echo "protocol-check: $GENERATED matches the wire types"
