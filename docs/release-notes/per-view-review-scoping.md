# Per-view review scoping (Local Branch vs PR Diff)

## What changed

A local PR tab reviews two different diffs — the **Local branch** (Branch / Unstaged /
Staged / History views) and the **PR head-vs-base** (PR Diff view). Review artifacts are
now scoped to the view they belong to, instead of being shared across both:

- **Split per view:** triage, `review.json`, professor / experts, questions / notes,
  `reviewed` markers, and the checklist. Generating triage (or any review) while viewing
  the PR Diff no longer leaks into the Local branch view, and vice versa.
- **GitHub PR comments** stay **shared** between the Local branch and PR Diff views (they
  belong to the PR and two-way sync with GitHub) and are hidden in Unstaged / Staged /
  History, where PR-line anchors don't apply.
- **Guided tour (Guide):** the Local branch and PR Diff each keep their own tour, but a
  tour whose diff matches the active diff is reused across both — so when the branch and
  PR diffs are identical, generating one Guide serves both, and once they drift each view
  keeps its own (shown stale with "Re-run guide"). The Guide tab stays context-aware (it
  can show the PR tour and the Diff toggle returns to the correct diff).

## Why it's safe

Storage was already per-view-bucket for Unstaged / Staged / History; this makes the Branch
view consistent with them and gives the PR Diff view its own PR bucket. GitHub comments
resolve to the shared PR bucket regardless of view, so two-way sync is unaffected. The tour
reuses the existing context machinery (`tour_context_is_pr`, `tour_stale`); only the
storage and selection moved to per-view buckets with identical-diff reuse.

## Implementation

- `crates/er-engine/src/app/state/mod.rs` — `apply_managed_root` routes only the PR Diff
  view to `prs/pr-<N>/`; Branch falls through to `branches/<b>/view-buckets/branch/` (with a
  safe fallback when the PR slug can't be resolved). New `github_comments_dir()` /
  `pr_bucket_er_dir()` keep GitHub comments PR-scoped; `reload_ai_state` hides them outside
  the Branch / PR Diff views. `resolve_view_tour()` picks the per-view bucket via
  `tour_context_is_pr()` and reuses a matching-diff tour across buckets.
- `crates/er-desktop/src/commands.rs` / `snapshot.rs` — `generate_tour` writes `tour.json`
  to the active view's bucket; the tour snapshot carries `fresh` + `scope`.
- Also fixes: recompute `branch_diff_hash` when a tour exists (so a fresh Guide isn't shown
  stale), and a deterministic storage fallback when the PR bucket can't be resolved.
- Tests cover the artifact split, GitHub-comment scoping, and per-view tour reuse.
