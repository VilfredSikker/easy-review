#!/bin/bash
# usage: locked.sh <worktree> <command...>
# Runs a heavy check one at a time across all agents, with a shared cargo
# target dir and a fresh temp HOME; flock -o keeps the lock out of child
# processes, so an orphaned test child cannot hold it; reports if the temp HOME got app data.
wt=$1; shift
cd "$wt" || exit 2
th=$(mktemp -d /tmp/er-home.XXXX)
HOME=$th CARGO_HOME=/root/.cargo RUSTUP_HOME=/root/.rustup \
  CARGO_TARGET_DIR=/home/user/er-v1/targets/$(basename "$wt") \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  PATH=/root/.cargo/bin:/root/.bun/bin:/opt/node22/bin:/usr/local/bin:/usr/bin:/bin \
  flock -o /home/user/er-v1/.lock "$@"
rc=$?
if [ -e "$th/.local/share/easy-review/v1/app" ] || [ -e "$th/Library/Application Support/easy-review/v1/app" ]; then
  echo "locked.sh: WARNING temp HOME $th holds easy-review/v1/app" >&2
fi
echo "locked.sh: exit $rc (HOME=$th)" >&2
exit $rc
