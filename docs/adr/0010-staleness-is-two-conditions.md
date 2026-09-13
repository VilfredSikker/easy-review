# Diff staleness and anchor staleness are separate conditions

A finding is stale when the diff it was generated against no longer hashes to the same value — the sidecar carries the SHA-256 of that diff, so a mismatch means the finding may describe code that has since changed. A comment is stale when its anchor line can no longer be found in the current diff. A line that moved is *not* stale: the anchor is relocated to its new position and marked `relocated`, while an anchor whose line is gone is marked `lost` and flagged stale. `er` tracks and reports the two independently, because they answer different questions and have different remedies — a stale finding is regenerated, whereas a comment whose line disappeared needs a decision from the reviewer, since there is nothing left to re-anchor to. Collapsing them into one flag left the UI unable to say which had happened.

## Consequences

- Comment staleness is runtime-only and recomputed on load by relocating each anchor against the current diff; its persisted counterpart is `anchor_status` (`original` / `relocated` / `lost`), not a boolean. Finding staleness derives from the diff hash stored in the sidecar. The two live in different storage paths, so anything that unifies them has to change both.
- Tour staleness is a third instance of the same pattern and is likewise kept out of `is_stale`: a tour is generated against one view's diff and drifts on its own, so it carries `tour_stale` per context.
