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
