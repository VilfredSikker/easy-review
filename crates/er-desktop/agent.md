# er-desktop — traps and routing

Tauri backend for the desktop app: commands, snapshots, caches, background threads,
the embedded browser, persistent tabs/projects, PTY terminals, native notifications.
It adapts the engine's `App` and owns nothing the engine could hold.

Transport decisions live in the ADRs. Read them instead of a summary here:

- `docs/adr/0011-push-revision-not-polling.md` — the backend pushes revision events;
  the 30s timer is a fallback. A view that goes stale means an emit is missing, and
  shortening timers will not fix it.
- `docs/adr/0012-differential-snapshots.md` — `hunks_omitted`, `record_sent_file`, and
  the `get_snapshot` map reset that a from-scratch rebuild needs.
- `docs/adr/0016-split-content-and-chrome-revisions.md` — content, chrome, and reviewed
  revisions are separate; which one a change bumps decides what the frontend rebuilds.
- `docs/adr/0015-off-main-thread-tauri-commands.md` — commands that wait on the app lock
  run off the main thread.
- `docs/adr/0018-app-cmd-ingests-snapshots.md`, `docs/adr/0017-optimistic-frontend-writes.md`
  — how the frontend sends mutations and applies what comes back.

`src/snapshot.rs` is the wire contract for `desktop-ui/src/lib/types.ts`. Both sides
compile without the other, so a changed field is invisible until runtime.

## Traps

**PTY output never touches the snapshot and never bumps a revision.** Terminal output
streams over `terminal-output` / `terminal-exit` events only. Nothing waits on a
revision to notice a session, and terminal contents cannot be restored from a
snapshot: whatever the frontend did not buffer from the stream is gone.
`docs/adr/0019-terminal-output-out-of-band.md`.

**Profiling needs both gates.** `ER_DESKTOP_PROFILE_POLL=1` prints nothing unless the
`profile` group also passes the `ER_LOG` filter — `profile_log::profile_log` checks
both. In the webview the switch is `localStorage.setItem("erProfilePoll", "1")`, and
its output goes to the devtools console, not the Tauri terminal.

**`localhost` and `127.0.0.1` are different cookie origins.** Keep `localhost` in
browser URLs and in the proxy's default authority; swapping one for the other splits
the cookie jar and breaks OAuth.

**Proxy redirects are pass-through by design.** A redirect `Location` is rewritten to
`erp(s)://` and handed back to the WebView so the next hop runs in the browser's
cookie jar. Never follow a redirect chain in ureq, never strip the handshake query
before the app has handled it, and never add provider-specific URL checks — the policy
is provider-agnostic. `crates/er-desktop/src/browser_proxy.rs`.

**Overlay z-order depends on which browser path is live.** The native child webview is
composited above the Svelte shell, so no modal can paint over it —
`browser_suspend_for_overlay` destroys the child webviews when an overlay takes focus,
because on macOS `hide()` leaves them stealing clicks. The `erp://` iframe fallback is
a DOM element, and the shell layers over it normally.

**Re-activating an already-open remote PR skips tab persistence.**
`activate_or_open_remote_pr` returns from its fast path after setting `active_tab`,
without calling `persist_app_tabs`, so the new active index is lost on restart.
`crates/er-desktop/src/commands.rs`.

## Rules with consequences

- Keep `App` lock scopes small: capture context, then run `gh`, `git`, or an agent
  subprocess outside the lock.
- `pr_cache`, `gh_status_cache`, `loading`, `watch_status`, `terminals` and
  `pending_ai_replies` are desktop-owned. A change to them usually needs a
  `desktop_revision` bump, or the frontend never hears about it.
- Backend state that changes without an emit is invisible until the fallback timer
  fires, which is why ~30s staleness is the signature to look for.
- Snapshots carry plain `text`. Never generate syntax spans in `build_snapshot`;
  Shiki runs in the frontend worker.
- Call `persist_app_tabs` after any new path that mutates `app.tabs` or
  `app.active_tab`. Never from `poll` / `get_snapshot`.
- `submit_github_review` is high risk: submit only valid, unsynced local comments, and
  mark them synced only after GitHub confirms.
- Reviewing a PR must not touch the user's worktree; use fetched refs and the PR tab
  constructors. `docs/adr/0020-read-only-pr-review.md`.

## Paths

Storage root is `$ER_STORAGE_ROOT` or `<platform data dir>/easy-review` — on macOS
`~/Library/Application Support/easy-review`. Sidecars resolve through the engine
(`TabState::apply_managed_root()`, `er_engine::storage`); a hand-built path is how the
desktop and TUI drift apart. There is no storage module in this crate.
`docs/adr/0003-managed-review-storage.md`, `docs/adr/0004-per-view-artifact-scoping.md`.

`src/export.rs` is a re-export shim over `er_engine::export`; the Markdown renderer
lives in the engine.

Dev log groups are defined in `crates/er-desktop/src/dev_log.rs`; select them with
`ER_LOG=<groups>` or `--logs <groups>`. `./scripts/tauri-dev.sh --logs arena` sets the
filter for Vite and Rust together.
