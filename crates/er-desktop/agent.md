# er-desktop Agent Guide

`crates/er-desktop` is the Tauri backend bridge. It adapts the engine's `App` into a desktop app by adding commands, snapshots, caches, background threads, browser proxying, persistent tabs/projects, PTY terminal support, export, and managed review storage.

## Main Files

- `src/commands.rs`: Tauri command surface. Most frontend actions enter here.
- `src/snapshot.rs`: Rust wire contract for `desktop-ui/src/lib/types.ts`.
- `src/main.rs`: Tauri setup, browser proxy/content script, background loops, command registration.
- `src/tabs.rs`: persisted desktop tab descriptors and tab reconstruction (`tabs.json` under the platform config dir).
- `src/projects.rs`: persisted project list, tracked branches, tracked/dismissed PRs.
- `src/pr_cache.rs`: GitHub PR list fetching/caching helpers.
- `src/export.rs`: pure Markdown renderer for comments, questions, findings, and UI annotations.
- `src/er_storage.rs`: desktop-managed review revision storage under app data.
- `src/terminal.rs`: PTY session wrapper for the in-app terminal drawer.

## Ownership Rules

- `AppState.app` is the engine state behind a mutex. Keep lock scopes small.
- `pr_cache`, `gh_status_cache`, `loading`, `watch_status`, `terminals`, and `pending_ai_replies` are desktop-owned state. Mutations here must be reflected in snapshots and usually need `desktop_revision` bumps.
- Syntax highlighting is client-side (Shiki Web Worker). Snapshots carry plain `text` only — never add span generation back into `build_snapshot`.
- Network/subprocess operations should run outside the `App` mutex. Capture context first, then run `gh`, `git`, or agent commands in the background.

## Snapshot Contract

`build_snapshot` is the bridge from Rust to Svelte. When adding a field:

1. Add the Rust `Serialize` type or field in `snapshot.rs`.
2. Populate it from engine or desktop-owned state.
3. Add the matching TypeScript type in `desktop-ui/src/lib/types.ts`.
4. Ensure missing/default values do not break older frontend assumptions.
5. Confirm polling revision changes when the field can change asynchronously.

## Tab persistence

Open tabs (repo, branch/PR identity, active index, optional `local_branch_diff_ref` and browser fields) are written to `tabs.json` via `tabs::persist_app_tabs` whenever the tab strip changes: open/close/reorder, branch or PR open (`place_tab`), project switch, tab select, and force-refresh (updates the refreshed branch ref). The same save runs on main-window `CloseRequested` and app exit as a safety net.

On launch, `main.rs` restores from `tabs.json` when present (eager diff for the active tab only; other tabs are lazy stubs). Call `persist_app_tabs` after any new code path that mutates `app.tabs` or `app.active_tab`. Do not persist from `poll` / `get_snapshot`.

## Polling And Invalidation

`poll` drains per-tab commands and app-level background tasks, computes a revision, and returns `snapshot: null` when unchanged. The revision currently combines engine state with `desktop_revision`.

### Idle CPU profiling (`ER_DESKTOP_PROFILE_POLL=1`)

Opt-in stderr profiler in `src/profile_log.rs`. Each line includes `kind`, `ts_ms`, `since_last_ms` (cadence vs last same kind), plus kind-specific fields.

```bash
ER_DESKTOP_PROFILE_POLL=1 cargo tauri dev 2>&1 | tee /tmp/er_profile.log
```

Kinds: `meta_refresh`, `rev_bump`, `revision_emit`, `poll` / `poll_skip` / `poll_revision_change`, `build_snapshot`, `get_snapshot`, `bg_loop`.

Frontend (devtools): `localStorage.setItem("erProfilePoll","1"); location.reload()` — logs `[er-profile]` for `revision_event`, `poll_invoke_*`, `snapshot_replace`, `highlight_*`, `span_keys_evicted`, `dev_height_fix`.

Bump `desktop_revision` when changing:

- PR list cache or PR refresh loading state.
- GitHub status cache or in-flight status flags.
- GitHub comment sync loading/result state.
- Watcher status.
- Background-thread results that do not mutate `App` directly.

## Feature-Specific Notes

- Background AI review tasks live in the engine `App`, not the active tab, so they survive tab switches. `commands::run_ai_review` should use `spawn_background_review` for review actions.
- Read-only PR review should use fetched refs and `TabState::new_local_pr`/remote PR tabs. Avoid `gh pr checkout` as a default review path.
- `submit_github_review` is high risk. Validate that only valid, unsynced local GitHub comments are submitted and only mark comments synced after GitHub success.
- Browser annotations cross the browser proxy (`src/browser_proxy.rs`), injected content script, `ui-annotations.json`, and snapshot reloads. URL canonicalization and re-anchor freshness are part of the contract. See **Embedded dev browser** below.

## Embedded dev browser (annotations-first)

The Browser tab’s primary surface is a **native child webview** (`review-browser`) loading real `http://localhost` URLs so WKWebView handles OAuth/cookies like a normal browser. The main window is transparent over the browser pane; `AnnotationOverlay` stays in the Svelte shell on top.

| Piece | Role |
|-------|------|
| `src/browser_webview.rs` | Create/position/hide child webview; `browser_host_message` / `browser_send_to_page` IPC |
| `src/frame_script.rs` | Injected script: `reportToHost()` → Tauri invoke or `postMessage` fallback |
| `desktop-ui/.../browserHost.ts` | `listen('browser://message')`, bounds sync, outbound eval |
| `src/browser_proxy.rs` | **Fallback** `erp://` / `erps://` proxy when native webview unavailable or iframe mode |

**Proxy navigation policy** (provider-agnostic) — `browser_proxy.rs`:

| Request | Behaviour |
|---------|-----------|
| Assets, `POST`, etc. | Single upstream hop, forward WebView headers |
| `GET`/`HEAD` document, same-origin 3xx | HTTP `Location` as `erp(s)://…` |
| `GET`/`HEAD` document, cross-origin 3xx | HTML `location.replace` to `erp(s)://…` (iframes ignore custom-scheme `Location`) |
| OAuth loops | Single server hop only; never follow redirect chains in ureq |

Do not add provider-specific URL checks. Use `localhost` consistently (`127.0.0.1` is a different cookie origin).
- `er_storage` redirects Desktop review artifacts away from the repo `.er` directory. Bootstrap existing `.er` data, then write active Desktop output to the managed revision path.
- Terminal sessions are OS resources. Dropping the stored `PtySession` kills the child shell; be careful with session id reuse and tab close behavior.

## Common Failure Modes

- UI does not update after background work: missing `desktop_revision` bump or revision hash input.
- App freezes: holding `App` mutex during network/subprocess work, expensive `build_snapshot`, large highlighted diff payloads, or oversized proxy responses.
- PR review mutates user worktree: accidental `gh pr checkout` or direct branch checkout path.
- Error visible only in UI: missing backend `log::error!` with durable context.
- Frontend type drift: Rust snapshot changed but `desktop-ui/src/lib/types.ts` did not.
