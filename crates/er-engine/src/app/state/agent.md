# app/state — agent guide

Which state lives where, and the traps. The tree shows the files.

## App vs TabState

`TabState` is one review target; `App` owns what outlives a tab switch: the open tabs
and active index, the AI provider/model selection, and background review tasks.

The AI split is deliberate. A *review* run keeps going while the user navigates
elsewhere, so it lives on `App` and reports back by target match. Summary, questions
and validate stay tab-local (`command_rx` / `command_status`), because they finish
against the diff they were started on — do not promote them.

State only one front end reads belongs in that front end: the desktop's caches live on
`crates/er-desktop::AppState`.

## Background review tasks

`BackgroundTaskTarget` is the identity, `kind` is the intent. Dedup is keyed on the
pair of `kind` and `target`, and is checked against running tasks and the pending
queue only: a finished task is never consulted, so it cannot block a respawn. Two
kinds on one target run side by side; `kind_label` is what keeps them apart in the UI.

`TabState::matches_target` is looser than the id on purpose. Repo, PR number, remote
slug and branch label decide it; `scope`, `base_branch` and `er_dir` are ignored. So
status and logs from a task started in one view bucket surface in every tab whose
label matches. Tighten it only with a destination for tasks that would then reach no
tab at all.

Session-only, in every direction. A finished task stays in `background_tasks` for 8s
(the toast window), then is retired to `recent_background_tasks`, capped at 32.
Per-task log ring 500 entries, a tab's `agent_log` 5000. The poll path drains the log
and result channels and retires; nothing here persists across restarts.

## Comments

Three stores, one per audience (`docs/adr/0007-three-comment-stores.md`); replies are
flat (`docs/adr/0008-flat-comment-threads.md`). The unified thread shape the desktop
draws (`ThreadSnapshot`, `crates/er-desktop/src/snapshot.rs`) is a desktop type — an
engine-side one would put a single front end's needs into the shared model. A new
comment-like entity needs its storage file, staleness rule, sync behavior and export
behavior decided before it is written.

## Traps

- **Never widen a field so a second front end can read something else out of it.**
  The engine compiles for both, so nothing flags it, and it breaks at runtime in
  whichever front end the change was not written for. Add an explicit field, or
  translate in the front end. `docs/adr/0006-engine-state-is-the-ui-contract.md`.
- **Continuous scroll.** The desktop draws every file in one scroll stream and derives
  the visible file from scroll position, in the frontend. The engine cursor still
  names one focused file. Engine work keyed on the cursor does not follow what the
  user is looking at, and a view that trusted the cursor would jump on every scroll.
- **Cursor movement stays cheap.** A plain move must not rebuild all diff rows; the
  desktop snapshot contract is the only reason to.
- **`DiffMode` doubles as the artifact scope** — eight variants, including the PR and
  tour views, so a mode change can move the managed root and reload the AI state.
  `docs/adr/0004-per-view-artifact-scoping.md`.
