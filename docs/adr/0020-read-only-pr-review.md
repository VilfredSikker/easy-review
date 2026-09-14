# Reviewing a pull request never checks out the user's working tree

Opening a PR fetches its head and base refs and diffs them where they lie. The working tree is untouched, in the TUI (`--pr`, PR URLs) and in the desktop alike, and `DiffMode::PrDiff` is a view over refs rather than over the checked-out branch. Checkout-based flows exist, but only when the user asks for one; nothing a review opens may move the user's HEAD on its own.

A review tool that mutates the tree it is reviewing breaks in ways that surface late. A checkout moves the user's branch, can lose uncommitted work, and — the quiet one — swaps the diff underneath the annotations being written against it. Comments, findings, and tours are anchored by file and line; if opening a PR rewrites the working tree, those anchors describe content that is no longer on screen, and the staleness that results looks like a bug in staleness detection rather than in the checkout.

## Consequences

- The local branch and the PR head are two real diffs, not one diff with a source flag, so they stay two view buckets (`Branch` vs `PrDiff`) with separate sidecars. Collapsing them would have to answer which tree a line anchor belongs to.
- A new PR entry point added to either front end must reuse the ref-reading path. Reaching for `gh pr checkout` (or any `git checkout`) to "get the files" reintroduces the mutation this decision exists to prevent.
