#!/bin/sh
# Fail when a fixed atomic-write temporary name returns.
#
# A fixed "<file>.tmp" name lets two writers of one path consume each other's
# file during the rename. Use er_store::atomic::write_atomic, or a unique
# "<file>.tmp.<pid>.<counter>" name. Comments are skipped, and unique names
# (whose literal carries ".tmp.<pid>.<n>") do not match.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

hits=$(
	grep -rnE 'with_extension\("[^"]*tmp|format!\([^)]*\.tmp"' crates --include='*.rs' |
		grep -vE '^[^:]+:[0-9]+:[[:space:]]*(//|/\*|\*)' || true
)

if [ -n "$hits" ]; then
	echo "atomic-tmp-check: fixed temporary file names found:" >&2
	printf '%s\n' "$hits" >&2
	echo "Use er_store::atomic::write_atomic, or a unique <file>.tmp.<pid>.<n> name." >&2
	exit 1
fi

echo "atomic-tmp-check: ok"
