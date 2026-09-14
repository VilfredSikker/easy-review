# Poll invalidation is derived from the snapshot fields, not declared by writers

`compute_content_revision` and `compute_chrome_revision` hash an explicit list of snapshot fields. `poll_impl` recomputes both and compares them against the `last_sent_*` markers to decide whether to send anything at all. Nothing declares "this changed"; the hash of the fields is the declaration.

The alternative — a version counter every writer bumps — was not taken because it is easy to forget, and a forgotten bump is silent. This design fails the other way: a snapshot field that is not added to the hash list will never be noticed, and the failure has no recovery path. The poll keeps comparing equal, so it keeps answering `snapshot: null`, and the 30-second fallback does not help because the fallback calls the same comparison.

`desktop_revision` is itself one of the chrome hash inputs. That is the whole reason a bump-only change — one where a writer touched something outside `App` and bumped the counter instead of the data — ever reaches the frontend. Removing it looks like deleting a redundant, ever-changing input; it silently breaks every update that relies on a bump.

## Consequences

- **Adding a snapshot field means adding it to the revision hash.** Otherwise the field is invisible on the wire, and no test that asserts on a direct command response will catch it — only a poll-driven path will.
- Prefer data changes over `desktop_revision` bumps where both are possible, since a bump resends chrome for everything.
- ADR 0016's separate content and chrome revisions are the other half of this: the hash lists are what make the split meaningful, because each one decides what gets rebuilt.
