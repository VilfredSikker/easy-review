# Single-owner view state lives in its front end

_Supersedes ADR 0006 for view state that only one front end reads._

## Context

ADR 0006 made the engine state model the contract between the TUI and the
desktop, and kept fields only one front end reads on purpose. Phase 4 moves
domain state behind the Workspace actor (ADR 0067). Fields that only the
desktop reads would ride along into the service, which has no use for them.

## Decision

View state with one front-end owner lives in that front end. The desktop's
per-tab review browser (URL, layout, split ratio, annotate mode, tooltips) and
its panel visibility left `TabState` and `App` for
`crates/er-desktop/src/view_state.rs`. desktop-ui's store accessors, command
names and snapshot payload did not change. ADR 0006 still holds for state both
front ends read, and D20 keeps the TUI layout widths, selection, cursor and
scroll in the engine until phase 5b.

**Browser state is keyed on a tab key.** `TabState` carries a `TabKey`, a
process-local number given when the tab is built and never saved. On the
`v1-service` path the app registry issues the key and the App's tab adopts
it, so the registry, the App, the loaded content and the view state name a
tab by one key. The type
lives in the engine half that always compiles, so the desktop and the service
name the same key.

**`tabs.json` is unchanged.** Save copies each tab's URL and layout from the view
state into its descriptor; restore copies them onto the rebuilt tab's new key.
Split ratio, annotate mode, tooltips and panels stay in memory, as before.

## Considered Options

- **Key on the tab's descriptor** (repo root, kind, branch or PR). Two open tabs
  can share one: a new tab (Cmd-T, the tab strip "+") opens a Working tab on the
  active repo, and nothing dedupes it. Both would show and save one URL.
- **Descriptor plus an occurrence number.** Close and reorder would have to
  shift the map, which spreads view-state code into the tab-list commands.
- **Let duplicate tabs share.** A visible change: a new tab would open showing
  its neighbour's page.

## Consequences

- The view lock is taken after the engine lock, never before, and is released
  before a snapshot build takes it again.
- Source-scan tests keep the six fields out of the engine and the service, and
  keep the view-state module from writing to disk.
- Selection and navigation live in desktop-ui on the `v1-service` path (P57,
  ADDENDUM-06's A10). The selected file and hunk are page state, and
  `select_file`, `next_file`, `prev_file`, `next_hunk`, `prev_hunk` and
  `jump_to_unreviewed` run as functions over `diff(tab)` and `reviewed(tab)`
  (`desktop-ui/src/lib/navigation.ts`), so a key press makes no IPC call. This
  moves the desktop's copy ahead of D20's phase 5b, because the App leaves
  er-desktop in phase 5. The engine's `navigation` functions stay for the TUI,
  and one fixture (`crates/er-engine/tests/fixtures/navigation_cases.json`)
  holds both to the same answers, so the two cannot drift.
- The TUI's view state lives in er-tui on the `v1-service` path (ADR 0073).
  Per tab key, `TuiView` holds what the App's `TabState` holds for the TUI
  today: selection, cursor, scroll, panel, layers, filter, layout widths and
  the comment draft. `session.json` stays the TUI's file in the tab's
  bucket. The flag path saves it from the `TuiView` and restores it on the
  same branch diff hash, with the draft's client id, so the App path and the
  flag path read each other's sessions. The service never reads it.
