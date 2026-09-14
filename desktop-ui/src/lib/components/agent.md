# Components

Thin views over `app.snapshot` plus `app.cmd` calls. The tree shows what exists; this file is the traps.

## The diff

[`FlatDiffView.svelte`](FlatDiffView.svelte) owns the diff: windowing, split and unified modes, inline findings and threads, selection and composers. [`DiffView.svelte`](DiffView.svelte) is an 11-line wrapper that forwards `viewModeOverride` to it. Edit `FlatDiffView`.

## Modals

Route every modal through [`ui/ModalShell.svelte`](ui/ModalShell.svelte). It owns escape/backdrop close, focus restore, and `overlay.acquire()` — acquiring is what hides the native review-browser webview, which no z-index can cover. A hand-rolled modal leaves a panel the webview can sit over and steal clicks from.

## `$effect`

- Track **one dependency** per effect (`open`, `visible`). Read unstable prop callbacks outside the body: `onClose={close}` is fine, `onClose={() => …}` read in the body is not.
- Wrap writes to external stores and overlay depth in **`untrack()`**. An effect that reads the state it writes depends on its own side effect and reschedules itself: for `overlay` that is an infinite loop with depth runaway, and for [`diffFileCollapse.svelte.ts`](../stores/diffFileCollapse.svelte.ts) it wedges Svelte's reactive flush, so chevrons, checkboxes and sticky headers stop updating until remount. The collapse mutators untrack internally so callers need not; `ModalShell` untracks acquire/register for the same reason.
- `onMount`/`onDestroy` for one-time global listeners, `$effect` when dependencies change. Always clean up: `removeEventListener`, `clearInterval`, `clearTimeout`, abort flags, `ResizeObserver.disconnect`.
- Keep non-reactive stacks/maps for modal ids and dismiss callbacks; `$state` only what renders (overlay `#depth`, which hides the webview).

## State traps

- `agent_commands` and `agent_log` are the **active tab's**; `background_tasks` is **session-wide, across all tabs**. `AgentOutputCard` reads the first pair, `BackgroundTasks` the second.
- **Effort-gated activation**: picking a model that advertises `effort_levels` opens an effort submenu rather than activating it. `set_ai_selection` fires with the chosen `effort`, or immediately when the model has none (`effortChoicesForModel`).
- **Reviewed state is read live from the snapshot** (`files[].reviewed`, `reviewed_count`), never copied into local state: `app.cmd` mutates those in place optimistically and rolls back on failure (ADR 0017, ADR 0016).

## Test seam

Components default to `app.snapshot` and let an explicit prop win, which is how stories and tests inject state — `viewModeOverride` (`FlatDiffView`, else `app.diffViewMode`), `filesOverride` (`FileTree` picker), `pinnedOverride` (`LeftSidebar`), `tabs`/`active` (`TabStrip`). Follow the pattern for a state the snapshot cannot supply.

## Writes

Mutations go through `app.cmd` so the returned snapshot is ingested and failures reach the toast, banner and log; raw `invoke` is for commands whose result must not replace the live snapshot. See ADR 0018. Do not build command names dynamically from user input.
