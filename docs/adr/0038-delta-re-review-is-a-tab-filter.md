# Delta re-review is a Tab filter, not a Tour

When a review is Stale, the expensive look is the next commit, not the first
walk. The person is asked only for files whose per-file hash moved, Findings
whose `finding_key` moved, and a small sample of claims that did not. That set
lives on the Tab and filters `visible_files`. It is not a Tour: a Tour that no
longer matches its diff is regenerated, never repaired (ADR 0025).

The sample is mandatory. Skipping unchanged files without it would trust silent
persistence forever, which is how a bug that was always present and is only now
reachable gets missed.

Agents leave `file_hashes` empty on `review.json`. The host writes a baseline
next to the review (`review-hashes.json`) while the review is still fresh, so
the skip has something to compare when the diff later moves. That file is
mechanical, not a Finding, and is not a second review sidecar.

## Considered Options

- **A new `DiffMode`.** Same shape as Tour. Rejected because this flow is not a
  narrative and must not hide the file tree or regenerate a Tour in place.
- **Auto-mark skipped files Reviewed.** Reviewed already clears when a file hash
  moves, but the mark means the person finished with the file. A skip they did
  not look at is a different claim.
- **Rewrite `file_hashes` into `review.json`.** The field is mechanical, but
  `review.json` is AI-owned. A host-written companion keeps the skip without a
  fourth writer on that file (ADR 0035).
