# v0.6.0 build decisions

Decisions taken while building the v0.6.0 packages that a reviewer would
otherwise have to dig out of a PR thread. One section per package.

## P1C: Inbox main view and the open-in-new-tab modal

- **The "always open in a new tab" preference lives in localStorage**
  (`er.alwaysOpenInNewTab`), not in `config.toml`. It only decides whether this
  window shows a dialog; it never changes what the engine does, so it does not
  belong in managed config (ADR 0005). Clearing it is the "Ask before opening"
  control in the Inbox footer, shown only while the preference is on.
- **The default is inverted for the Inbox only.** A plain open from the Inbox
  asks with the tab-open modal instead of silently replacing the active tab.
  Cmd/Ctrl-click and middle-click still open a new tab without asking.
- **Other open paths keep the old default.** The sidebar's PR and branch rows
  call `shouldReplaceTab`, which still replaces on a plain click. They do share
  the preference: once it is on, a plain sidebar click opens a new tab too.
  Wiring the sidebar to the modal belongs to the package that owns
  `LeftSidebar.svelte`.
- **Mark all read and Clear read act on the rows shown.** They sit beside the
  project and type filters, so they send `mark_inbox_items_read` and
  `clear_inbox_items` with the shown threads' ids rather than the global
  commands. Clear read removes only threads with nothing unread, so an unread
  thread keeps its earlier events.
- **The selected row stays listed while it is selected.** Selecting marks a
  thread read; under the Unread filter it stays in place until another row is
  chosen or the read filter changes, so the detail pane and Enter stay on the
  row the reader picked.

## P22: Top bar, running-AI pill and jobs popover

- **`TopBar` is built but not mounted yet.** `App.svelte` is outside P22, and
  `BranchContextBar` still carries controls the top bar does not: copy branch,
  worktree and PR links, the terminal and browser toggles, Sync for a stale
  diff and the local/PR source toggle. Swapping the bars now would drop all of
  them. The package that assembles the shell mounts `TopBar` in place of
  `BranchContextBar`, rehomes those controls, and removes the floating
  `BackgroundTasks` mount in the same change. Until then `BackgroundTasks`
  floats the one `RunningPill`, so the pill never shows twice.
- **No "Run standard instead" on the Deep confirmation.** The prototype offers
  it, but the engine cannot start a review at a chosen depth:
  `start_review_flow` takes only a scope and triage picks the depth, so
  declining and restarting lands on Deep again. The card offers Run deep
  review and Skip. The button needs an engine command that confirms the
  pending flow at standard depth; it is not faked from existing commands.
- **The pill drops mirrored agent commands.** The snapshot copies every
  background task on the active tab into `agent_commands` under its kind for
  the per-tab badges, so the pill removes commands whose name and status match
  a background task before aggregating. A tab-local command that shares a
  task's kind and status is hidden with it; the task row still shows the work.
