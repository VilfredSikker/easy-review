#!/bin/bash
# Guarded push for epic agents. Use instead of `git push`.
# usage: safe-push.sh <branch> <epic branch>
# Refuses when the push would rewrite history (not a fast-forward of the
# remote branch) or when the branch does not contain the current release tip,
# so a stale branch can never overwrite or revert work merged after it.
set -euo pipefail
branch=${1:?usage: safe-push.sh <branch> <epic branch>}
release=${2:?usage: safe-push.sh <branch> <epic branch>}
[ "$(git rev-parse --abbrev-ref HEAD)" = "$branch" ] || { echo "safe-push: HEAD is not $branch" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "safe-push: uncommitted changes; commit or finish the merge first" >&2; exit 1; }
git fetch -q origin "$release"
tip=$(git rev-parse "origin/$release")
if ! git merge-base --is-ancestor "$tip" HEAD; then
	echo "safe-push: $branch does not contain the current $release tip ($(git rev-parse --short "$tip"))." >&2
	echo "  Something merged since you last synced. Run: git merge origin/$release" >&2
	echo "  Resolve conflicts keeping both sides, re-run your checks, then safe-push again." >&2
	exit 1
fi
if remote=$(git ls-remote --exit-code origin "refs/heads/$branch" | cut -f1); then
	git fetch -q origin "$branch"
	if ! git merge-base --is-ancestor "$remote" HEAD; then
		echo "safe-push: the push would rewrite $branch's history (remote $(git rev-parse --short "$remote") is not in HEAD). Merge it instead; never force-push." >&2
		exit 1
	fi
fi
for i in 1 2 3 4; do git push -u origin "$branch" && exit 0; sleep $((2 ** i)); done
exit 1
