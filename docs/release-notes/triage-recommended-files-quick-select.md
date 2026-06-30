# Triage-recommended files as a quick select

## What changed

When Triage has flagged specific files to review (its `priority_files`), you can
now scope a review to exactly those files in one click — no manual file picking:

- **Review select files** — the file picker shows a **Triage (N)** quick-select
  button alongside *Mark all* / *Unmark all*. Clicking it narrows the selection
  to just the Triage-recommended files, then you choose reviewers as usual.
- **AI Review Arena** — the launcher shows a **Review N triage-recommended
  files** button under the Scope control. Clicking it sets the run scope to those
  files (equivalent to picking them in the "Selected" scope).

This is most useful on large PRs, where a full review across every changed file
is slow and costly. Triage does a fast first pass and surfaces the files that
matter; this turns that recommendation into the default review scope with a
single click.

The button only appears when there is a **fresh** triage (generated against the
current diff) that recommended at least one file. A stale triage hides it, so you
never accidentally scope a review to an out-of-date recommendation.

## Why it's safe

The recommendation is read straight from the existing `TriageSnapshot.priority_files`
already carried in the snapshot — no new backend command, no change to how
reviews or arena runs are dispatched. Both entry points reuse the paths-based
flows that already exist:

- The file picker validates Triage paths against the current view's diff
  (`list_diff_paths`), so only files actually present in the diff are selected.
- The arena button sets the same `selectedPaths` / `"selected"` scope the manual
  file picker already produces, so estimation and run dispatch are unchanged.

## Implementation

- `desktop-ui/src/lib/triageSuggestions.ts` — new pure helper
  `triageRecommendedPaths(triage)` returns the fresh, de-duplicated priority-file
  paths (empty for missing/stale/no-files triage).
- `desktop-ui/src/lib/components/AiReviewFilesModal.svelte` — adds the **Triage
  (N)** quick-select button (intersected with the loaded picker files). Works in
  both the review flow and the arena's "pick files" flow, since they share the
  modal.
- `desktop-ui/src/lib/components/arena/ArenaLauncher.svelte` — adds the **Review
  N triage-recommended files** button below the Scope control, with a
  selected/pressed state when the current selection already matches Triage.
- Tests: `triageSuggestions.test.ts` covers fresh/stale/missing/duplicate/empty
  cases.
