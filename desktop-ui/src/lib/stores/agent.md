# Stores

`desktop-ui/src/lib/stores/` — one store per file plus the pure helpers they import. The directory is the map. This file holds only the rules that span it.

## State ownership

- `app.snapshot` is backend truth at rest: render from it, read loading state from it (`app.switching`, `app.refreshing`, `snapshot.bg_loading`), never infer backend state from toasts.
- While a review write is in flight the store runs ahead of the backend on purpose. An optimistic write paints the record into `app.snapshot` and re-applies it over every incoming snapshot until the backend's own copy lands (`keepOptimisticOps`). The store is a second source of truth for in-flight review records — that is the point of the layer, and the pending-op list is what keeps the record alive across a poll.
- localStorage holds frontend preferences only: diff view mode, comment visibility, drawer sizes, scroll offsets. Never review data.

## Optimistic writes (ADR 0017)

- Contract: paint, register the op, confirm over IPC. `cmdOptimistic` applies the op before its first `await`, so a composer can close immediately; the op rolls back only if the call fails or the view identity moved on. A successful global op is never rolled back.
- Skipping the registration undoes the paint. Each snapshot replaces `app.snapshot` wholesale, and anything not held as a pending op is gone — the record flashes in and reverts.
- A command listed in `OPTIMISTIC_COMMANDS` with no branch in `buildOptimisticOp` returns null, and `cmdOptimistic` then drops the action: no paint, no IPC, no error. Add both.
- Paint gate: `canPaintOptimistic()` refuses while a tab switch is in flight. A refused paint calls `explainPaintBlocked()` and leaves the composer draft intact — a click that does nothing reads as a frozen app.
- Composers fire `void app.cmd(...)` and clear their draft after, never `await`.
- `optimisticChain` serializes optimistic IPC so a reply lands on the id its parent just created.

## Command wrapper vs raw invoke (ADR 0018)

Default is `app.cmd`: it ingests the returned snapshot, carries the tab-change ordering and the loading flags, routes failures to the banner + toast + log, and covers the optimistic commands.

Raw `invoke` is for commands whose result must not replace the live snapshot:

- a non-snapshot return — `export_review`, `get_background_task_log`, `list_available_branches`, the `arena_*` reads the arena store polls itself;
- the settings panel, which keeps its own `GetConfigHubResponse` state (`get_config_hub`, `apply_config_patch`, the AI provider editor) and bypasses the snapshot contract on purpose;
- `toggle_panel`, which paints the layout locally first.

A raw `invoke` reports its own failure; nothing else will.

## Ordering guards

Snapshots arrive out of order (poll, command response, revision event), so each path defends itself:

- `snapshotGeneration` — a poll that started before a newer command or merge is discarded (`isStaleSnapshotGeneration`).
- `tabChangeGeneration` + `shouldDropCommandSnapshot` — a command snapshot for another tab is dropped unless the command is a tab change; a failed tab change restores `lastConfirmedSnapshot`.
- `tabChangeInvokeQueue` (`createLatestInvokeQueue`) — tab-change invokes serialize, and a superseded one resolves to `LATEST_INVOKE_SKIPPED`, which the caller must ignore rather than apply.
- `pollInFlight` / `pollPending` — a revision event during a poll coalesces into one follow-up, not a second invoke.

Any new async path that assigns `this.snapshot` needs one of these.

## Poll model (ADR 0011)

The backend pushes `er://revision` when its desktop revision advances; the frontend answers by invoking `poll`. The 30s timer is a safety net for a dropped event or a listener that has not attached yet — it was 2s when polling was the primary mechanism. A stale-looking UI points at missing backend revision invalidation; shortening the interval hides that.

## Browser URL identity

Annotation matching keys on a canonical URL: use the `browserUrl.ts` helpers, never hand-parsed URLs in components. Dev URLs must stay on `localhost` — `127.0.0.1` is a different origin for cookies and for the proxy's same-origin redirect.

## Keyboard

One global handler in `keyboard.ts`; components register no global shortcuts and stop propagation in their own inputs. Guard scope: focus inside the terminal (`.xterm`) swallows everything except Cmd/Ctrl+T; bare keys stop at fields (`INPUT`/`TEXTAREA`/`SELECT`, contentEditable) and at an open modal; a few named chords opt out of the field guard. Escape is owned by the handler, first match wins: palette → modal stack → popover / search bar → diff selection → identifier highlight → annotation composer → blur the field.
