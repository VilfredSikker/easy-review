# AI reviews in Guide mode

## What changed

The Guide tab (the AI guided walkthrough — `tour.json`, internally `DiffMode::Tour`)
now supports the full set of AI review actions, matching the Diff views:

- Triage branch
- Run review / Run reviewers… (General, experts, Professor)
- Professor
- Generate / Regenerate tour
- Validate / re-anchor
- Review select files

Previously these were greyed out in Guide mode. Adding questions/notes and running
**Elaborate** already worked there; this brings the rest of the AI Hub in line.

## Why it's safe

Guide is not a separate diff — it is the **branch diff regrouped into pillars**,
built from the same `DiffFile` objects and sharing the same per-branch review
bucket (`ReviewBucket::Branch`). Findings, inline comments, questions, and notes
are keyed by file path (not by diff mode) and stored once per branch, so:

- A review run from Guide writes to the same bucket Diff reads, and vice versa.
- Findings and inline threads render in both views through the same pipeline.
- No data is duplicated or mode-scoped — Diff and Guide stay in sync automatically.

Running a review from Guide updates findings in place; it does **not** regenerate
the pillar grouping (use *Regenerate tour* for that).

## Implementation

- `desktop-ui/src/lib/reviewScope.ts` — `reviewScopeFromMode("tour")` now maps to
  the `"branch"` scope (was `null`, which disabled every review action). This
  single mapping re-enables the gated actions in the AI Action Palette and the
  Command Palette, since both key off `reviewScope != null`.
- `crates/er-desktop/src/commands.rs` — `resolve_review_scope` accepts
  `DiffMode::Tour` for the `"current"` scope (defensive; the frontend already
  passes an explicit `"branch"` scope).
- Tests: `reviewScope.test.ts` and `resolve_review_scope_accepts_tour_mode` cover
  the new mapping.
