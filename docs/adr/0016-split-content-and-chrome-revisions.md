# Content and chrome revisions are tracked separately

The desktop poll response carries independent `content_revision` and `chrome_revision` counters, plus a third, `reviewed_revision`, that is deliberately kept out of both. The frontend applies a snapshot when either of the first two changes, and a `chrome_only` response is merged over the hunks and spans it already holds rather than replacing them. With a single combined revision, every chrome change and every checkmark on a reviewed file looked identical to a changed diff — so marking a file reviewed paid to re-send and rebuild the diff.

## Consequences

- A chrome-only response may only merge onto a snapshot of the same view. One built for a different view (Branch vs PR Diff) carries empty `files`, so the frontend defers it and waits for a full snapshot; merging it would leave the previous view's diff on screen under the new view's tab.
- `reviewed_revision` exists so a reviewed-only change can be answered with `snapshot: null` and no rebuild. Anything that changes what the diff *is* bumps `content_revision`; anything that only changes how it is presented bumps `chrome_revision`. Bumping both puts the hunk rebuild back on the checkmark path.
