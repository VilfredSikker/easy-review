# er-desktop — traps and routing

Tauri backend for the desktop app: the three resource commands over er-service's
actors, the resources the desktop owns, the background loops, the embedded browser,
PTY terminals, native notifications, and the direct commands that are not published
state. It builds no `App` and owns nothing er-service or the engine could hold.

Transport decisions live in the ADRs. Read them instead of a summary here:

- `docs/adr/0072-the-desktop-protocol-is-versioned-resources.md` — the page reads
  resources by key and version, the backend pushes the keys whose version moved, and
  a 30s version check is the safety net. A view that goes stale means a writer that
  did not stamp a version, and shortening timers will not fix it.
- `docs/adr/0067-the-workspace-actor-and-the-crate-graph.md` — one actor per
  repo/worktree, effects off the actor thread, bounded reply waits.
- `docs/adr/0070-app-scoped-records-have-one-service-owner.md` — who writes the
  app-scoped records.
- `docs/adr/0015-off-main-thread-tauri-commands.md` — commands that block run off
  the main thread.

The wire types live in `er-protocol`, and `ts-rs` writes their TypeScript;
`just protocol-check` fails when the generated files drift. A type er-desktop sends
over IPC outside `er-protocol`, such as those in `src/page_view.rs` that
`desktop-ui/src/lib/pageTypes.ts` mirrors by hand, has no such check, so a changed
field is invisible until runtime.

## Traps

**PTY output never touches a resource and never moves a version.** Terminal output
streams over `terminal-output` / `terminal-exit` events only. Nothing waits on a
version to notice a session, and terminal contents cannot be refetched: whatever the
frontend did not buffer from the stream is gone.
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

**The backend tick drives what no page call does.** It asks each loaded tab's actor
to read its sidecars again (how an outside agent's write reaches the content), has
the task host retire finished tasks and start queued ones, and has the sidebar owner
publish. It wakes on each revision the watcher counts and on an interval floor. Each
duty publishes through its own resource, so the tick moves no desktop revision; a
duty that did would wake the tick again. `crates/er-desktop/src/resources/tick.rs`.

**Two diff hashes with different contracts.** `compute_diff_hash` is SHA-256,
persisted in sidecars; `compute_diff_hash_fast` is a `DefaultHasher` u64 rendered as
16 hex characters, for in-process change detection only — never persist it or compare
it across processes. A tab's diff section keeps `diff_hash`, which may be either, and
`branch_diff_hash`, always SHA-256, which is what the sidecars are checked against.
Comparing `diff_hash` against a sidecar's stored hash is the obvious thing to do in a
loader, and after a watch refresh it is a guaranteed mismatch that looks exactly like
staleness.

## Rules with consequences

- Keep lock scopes small: capture context, then run `gh`, `git`, or an agent
  subprocess outside the lock.
- A desktop write to state a resource serves must move that resource's version and
  wake the relay: through its `WriteClock`, the `SessionClock`, or its owner's
  publish. A write that skips that is never pushed, and the safety net does not save
  it: the version check finds the version unmoved.
- Resources carry plain `text`. Never generate syntax spans in the backend; Shiki
  runs in the frontend worker.
- The service's app registry is the one writer of `tabs.json` and
  `pr-open-cache.json` (`crates/er-service/src/app_registry.rs`). The desktop sends
  its tab and prefetch commands there and never saves either record itself.
- Reviewing a PR must not touch the user's worktree; use fetched refs and the PR tab
  constructors. `docs/adr/0020-read-only-pr-review.md`.

## Paths

Storage root is `$ER_STORAGE_ROOT` or `<platform data dir>/easy-review/v1` — on macOS
`~/Library/Application Support/easy-review/v1`. The `v1` segment keeps 1.0.0 data
away from an older install's files, which are left untouched. Sidecars resolve
through the engine (`er_engine::storage`) into the tab context er-service builds
(`er_dir` for the view bucket, `comments_dir` for GitHub comments); a hand-built path
is how the desktop and TUI drift apart. There is no storage module in this crate.
`docs/adr/0003-managed-review-storage.md`, `docs/adr/0004-per-view-artifact-scoping.md`.

The Markdown export renders in the engine's always-compiled `review_export`;
`src/export.rs` adds only the desktop's exclusion step.

Dev log groups are defined in `crates/er-desktop/src/dev_log.rs`; select them with
`ER_LOG=<groups>` or `--logs <groups>`. `./scripts/tauri-dev.sh --logs arena` sets the
filter for Vite and Rust together.
