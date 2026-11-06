# A review regrades triage without rewriting it

Triage is a first read made before anything is reviewed: its risk, priority files and "next" call are guesses. Once a review has graded the same diff, its findings — general and expert lenses alike — are the better answer, and the triage card kept showing the guess. The desktop snapshot now derives a review overlay on the triage it sends (`review_update`), and the card shows that overlay's risk, priority files and next step, keeping triage's routing call under Verdict.

The overlay is derived each time the snapshot is built, from the open findings and file risks of a review whose diff hash matches the current diff. Nothing writes it back to `triage.json`. Triage and the review are separate agent-owned sidecars (ADR 0009 for findings), re-run independently: a stored merge would go stale the moment either side reran, and a re-triage would silently discard what the review said. Deriving it means whichever ran last, the card shows triage as written plus what the newest review of this diff adds.

The alternative was to have the review flow rewrite `triage.json` when it finished. It was not taken because it gives the review flow a second sidecar to own, misses reviews run outside a flow (a single lens, an arena import), and loses the original routing call, which still explains why the review ran the lenses it did.

## Consequences

- A review of an older diff adds nothing: an overlay from another diff would be a less precise verdict, not a more precise one.
- Resolving or dropping findings changes the overlay on the next snapshot, so "Fix 2 high findings" counts down as the reader works.
- The overlay lives in the desktop snapshot (ADR 0006). The TUI still shows triage as written; giving it the same view means moving the derivation into `er-engine`.
