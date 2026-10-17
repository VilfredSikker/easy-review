# T09 — Non-destructive presentation and overlay leases

Status: ready-for-agent
Depends on: T02 (parking proven "yes"), T06 (browsers keyed by tab ID)
Size: 3–4 days
Risk: high (every modal in the app depends on this)

> **For the executor:** Do not start unless `proof-log.md` `## T02 parking` says
> **Result: yes** and names a variant. Build exactly this, in order.

## In plain terms

- **The problem.** Every time any dialog opens, the app destroys the live browser
  page so it cannot steal clicks, then tries to rebuild it. Typed form text, scroll
  and history are lost, and sometimes the pane comes back blank.
- **The fix.** One owner decides where each browser page is and whether it can be
  touched. Dialogs take a "lease" that parks the page (hidden and unable to take
  input, but alive) using the mechanism T02 proved. When the last dialog closes,
  only the page that should be visible comes back, in the right place.
- **TL;DR.** Dialogs hide the page instead of killing it.

## Definition of done

- [ ] With text typed and scrolled halfway in `form.html`, opening and closing any
      modal returns the same page: same `#doc-id`, text, scroll, Back history.
      The modal receives all mouse and keyboard input while open.
- [ ] Nested modals: the page returns only after the last one closes.
- [ ] Rapid tab switching (10 switches in 2 s) ends with only the selected tab's
      page visible and no focus stolen from the shell.
- [ ] Hiding the browser pane and showing it again keeps the page.
- [ ] `browser_suspend_for_overlay` and `reset_all_tab_webviews` are deleted.
- [ ] Tests in Step 6 pass.

## Before you start

- **Native park/unpark:** `crates/er-desktop/src/browser_native.rs` (`park`,
  `unpark`, the variant named in the T02 proof log).
- **Destroy path to remove** (`crates/er-desktop/src/browser_webview.rs`):
  `:257` `park_tab_webview` (move off-screen + `hide()`), `:269` `hide_all_webviews`,
  `:284` `reset_all_tab_webviews`, `:306` `show_tab_webview`, `:371`
  `browser_suspend_for_overlay`, `:386` `browser_hide`, `:408` `browser_set_bounds`.
- **Frontend:**
  - `desktop-ui/src/lib/stores/overlay.svelte.ts` — `acquire()` increments a depth
    counter and, on the first acquire, calls `prepareOverlayFocus()`, which invokes
    `browserSuspendForOverlay()` then focuses the window. The release closure only
    decrements depth.
  - `desktop-ui/src/lib/components/ui/ModalShell.svelte:49` —
    `const releaseOverlay = untrack(() => overlay.acquire());`.
  - `desktop-ui/src/App.svelte:168-173` — an effect calls `browserHide()` when the
    browser pane is closed.
  - `desktop-ui/src/lib/components/BrowserView.svelte`: `syncPaneBounds` (line 63)
    reads `getBoundingClientRect()` and calls `browserSetBounds`; a ResizeObserver
    in `onMount` (around line 657) triggers it; the effect starting at line 122
    keeps `prefillDone` set while blocked, which is why the pane can come back
    blank; `onDestroy` calls `browserHide()`.
- **Interface to implement** (from `reference/spec.md`, "Browser module and
  interface"): `attach(reviewTabId)`, `present(handle, presentation)`,
  `dispatch(handle, intent)`, `close(handle)`. This task implements `attach`,
  `present` and `close`; `dispatch` exists for navigation from T07.

## Scope

- **In scope:** a presentation owner in Rust (`crates/er-desktop/src/browser_presentation.rs`),
  `present`/`attach`/`close` commands, `overlay.svelte.ts`, BrowserView's
  existence/bounds effects, `App.svelte`'s hide effect, deletion of the destroy path.
- **Out of scope:** annotation code; navigation state (T07); dense layouts and
  shortcuts must keep working unchanged.

## Steps

### Step 1 — Presentation owner

- **Goal:** one place computes the final state of every child.
- **Why / invariant:** only the presentation owner calls `park`/`unpark`/set
  bounds. Any other caller is a bug, because racing callers are what made pages
  vanish.
- **Do this:** `PresentationOwner` holding, per tab ID: desired bounds (logical),
  visible flag; plus a global lease count and the selected tab ID. A single
  `apply()` computes: if leases > 0 → every child parked; else the selected tab's
  child unparked at its bounds if visible and bounds are non-zero; every other
  child parked. Run `apply()` on the main thread after each change; coalesce
  multiple changes in one turn into one apply.
- **Do this too:** put the native calls behind a small trait (`NativeChildren {
  park, unpark }`) so `apply()` can be unit-tested with a fake.
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" cargo test -p er-desktop browser_presentation`
  → the Step 6 unit tests that exist so far pass (write them alongside this step).

### Step 2 — Commands

- **Do this:** `browser_attach(tab_idx) -> BrowserHandle { tab_id, generation }`
  (creates the child only if missing; never navigates an existing one);
  `browser_present(tab_id, bounds: Option<Rect>, visible: bool)`;
  `browser_lease_acquire() -> lease_id` / `browser_lease_release(lease_id)`
  (release of an unknown or already-released lease is a no-op);
  `browser_close(tab_id)` (idempotent). `browser_lease_acquire` returns only after
  `apply()` has parked every child, so the caller can await it.
- **Verify:** `cargo check -p er-desktop` passes; from the devtools console,
  `await __TAURI_INTERNALS__.invoke('browser_lease_acquire')` makes the page stop
  receiving clicks before the promise resolves, and releasing it brings the page
  back with the same `#doc-id`.

### Step 3 — Modals take leases

- **Do this:** in `overlay.svelte.ts`, `acquire()` calls `browser_lease_acquire`
  and returns a release that calls `browser_lease_release` with that lease ID.
  `ModalShell` should not render interactive content until the lease promise
  resolves (keep the existing `untrack` wrapping so depth changes do not
  retrigger the effect). Delete `prepareOverlayFocus`'s suspend call; keep the
  window focus call.
- **Watch out for:** a release that arrives after the tab was closed or switched
  must not show that tab — `apply()` reads current state, not the state at
  acquire time.
- **Verify:** `cd desktop-ui && bun run test src/lib/stores` and `bun run check`
  pass (through `locked.sh`); in the app, opening Settings with `form.html` loaded
  keeps typed text after closing.

### Step 4 — BrowserView presents, never creates or destroys

- **Do this:** BrowserView calls `browser_attach` once per tab, then
  `browser_present` on bounds changes. Measure position as well as size: in
  addition to the ResizeObserver, re-measure on window resize, on pane split drag
  and once per animation frame while a layout transition runs; send at most one
  `browser_present` per animation frame, dropping superseded values. Delete
  `prefillDone`, the hide-on-blocked effect and the `browserHide` in `onDestroy`
  (replace with `browser_present(visible=false)`). In `App.svelte`, the
  pane-closed effect calls `browser_present(visible=false)` for the active tab.
- **Verify:** `grep -n "prefillDone\|browserHide" desktop-ui/src/lib/components/BrowserView.svelte`
  returns nothing; dragging the split divider keeps the page aligned with the pane
  edge throughout the drag (no lag of more than one frame visible).

### Step 5 — Delete the destroy path

- **Do this:** delete `browser_suspend_for_overlay`, `reset_all_tab_webviews`,
  `hide_all_webviews`, `park_tab_webview`, `browserSuspendForOverlay` in
  `browserHost.ts`, and their registrations in `crates/er-desktop/src/main.rs`.
  Closing a tab calls `browser_close` for that tab only.
- **Verify:** `grep -rn "suspend_for_overlay\|SuspendForOverlay\|reset_all_tab_webviews" crates desktop-ui/src`
  returns nothing; `cargo check -p er-desktop` and `bun run check` pass.

### Step 6 — Tests and real-window checks

- **Rust unit tests** on `PresentationOwner::apply` with a fake native layer:
  lease parks all; release of last lease shows only the selected tab; late release
  after tab close shows nothing for the closed tab; zero bounds never unparks;
  duplicate release is a no-op.
- **TS:** test that `overlay.acquire()` awaits the lease and release releases it.
- **Real window** (`./scripts/tauri-dev.sh`): every item in the definition of
  done, plus: open the command palette, settings, and a confirmation dialog in
  turn; each receives Escape and typing.
- Run `cargo test -p er-desktop browser_presentation`, `cd desktop-ui && bun run test`,
  `bun run check`, `just clippy`, `just lint-ui` (each through `locked.sh`).

## Rollback

Revert the commits; the destroy-on-modal path returns.

## When to stop and ask

- Any modal fails to receive input while a page is parked.
- A page returns at stale bounds after a layout change.
- You need a timer or retry to make parking reliable.
