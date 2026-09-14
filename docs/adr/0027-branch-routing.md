# Bug fixes target main; everything else targets the current release branch

Point a pull request at `main` only when it is a bug fix. Every other change is developed on and merged into the current release branch, resolved at use time as the highest `release/v*` on origin rather than named in the instructions.

Resolving it at use time is the decision. A hardcoded version is the one that just shipped by the time anyone reads it, so a pinned target quietly routes new work onto a finished line: the branch exists, the PR opens, and the work lands where nobody is releasing from.

## Consequences

- Nothing enforces this. CI does not check the change type, so a feature PR against `main` will pass. Docs and release chores do land on `main` in practice.
