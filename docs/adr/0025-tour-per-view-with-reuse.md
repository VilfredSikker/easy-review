# A tour belongs to the diff it was generated from

A guided tour is a narrative about specific changes, so once the diff moves underneath it the tour cannot be repaired — regenerating it is the only honest fix. That makes the diff hash its identity. Tours are therefore stored per view bucket, and a tour from the other bucket is reused when its diff hash matches the active diff: the local branch and the PR head are usually the same diff, and reuse means that case pays for one generation instead of two. Tour staleness is tracked separately from general staleness, per context, because the two have different baselines.

## Consequences

- The read crosses buckets while generation stays bucket-local — a new tour is written to the bucket of the view it was launched from — so nothing may assume the active bucket holds the tour being shown. Reuse reads the other bucket's file; it is never copied.
- Which tour the Guide shows follows the context the tab was opened from (`tour_is_pr`), not the diff mode alone, so the Guide keeps showing the PR tour after the diff mode changes under it.
- Staleness is per context: a branch-scoped tour baselines against the branch diff hash, which stays valid across Unstaged/Staged/History, so switching working-tree modes does not mark a fresh branch tour stale. When neither bucket is fresh, the stale tour is still returned so the Regenerate affordance has something to render beside it.
