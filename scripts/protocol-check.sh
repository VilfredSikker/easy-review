#!/usr/bin/env bash
# Fail when the committed TypeScript for the wire types, or the barrel that
# re-exports them, differs from a fresh export. The export goes into a
# temporary directory, so the check never writes the tree, and `diff -r` also
# reports a file present on one side only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GENERATED=desktop-ui/src/lib/protocol/generated
BARREL=desktop-ui/src/lib/types.ts
fresh="$(mktemp -d)"
trap 'rm -rf "$fresh"' EXIT

cargo run -q -p er-protocol --bin protocol-gen -- "$fresh/generated" "$fresh/types.ts"

status=0
if ! diff -ru "$GENERATED" "$fresh/generated"; then
  echo >&2
  echo "protocol-check: $GENERATED differs from the wire types." >&2
  status=1
fi
if ! diff -u "$BARREL" "$fresh/types.ts"; then
  echo >&2
  echo "protocol-check: $BARREL differs from the barrel of the wire types." >&2
  status=1
fi
if [ "$status" -ne 0 ]; then
  echo "Run \`just protocol-gen\` and commit the result; never edit these files by hand." >&2
  exit 1
fi
echo "protocol-check: $GENERATED and $BARREL match the wire types"
