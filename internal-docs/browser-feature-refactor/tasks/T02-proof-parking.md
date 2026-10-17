# T02 — Proof: park a live page under modals

Status: ready-for-human (needs real mouse and keyboard checks in the app window)
Depends on: T01
Size: 1–2 days
Risk: high (a wrong "yes" here breaks every modal in the app)

> **For the executor:** This is an experiment with a yes/no outcome. Build the
> smallest code that answers the question, record the answer in
> `internal-docs/browser-feature-refactor/proof-log.md` under `## T02 parking`,
> and stop. Do not wire the result into the product; T09 does that.

## In plain terms

- **The problem.** The browser page is a native view drawn on top of the app's
  HTML. When a dialog opens, the page would sit over it and steal clicks, so the
  app currently **destroys** the page and recreates it afterwards — losing form
  text, scroll and history, and sometimes leaving the pane blank.
- **The question.** Can we hide the native page so it receives **no** mouse or
  keyboard input, while keeping the same live page, and bring it back intact?
- **TL;DR.** Prove "hide without destroying" works reliably, or prove it does not.

## Definition of done

- [ ] `proof-log.md` `## T02 parking` records environment, the mechanism tried,
      and Result: **yes** or **no**, with observations for each scenario below.
- [ ] If yes: a module `crates/er-desktop/src/browser_native.rs` exposes
      `park(&tauri::Webview)` and `unpark(&tauri::Webview, LogicalPosition<f64>, LogicalSize<f64>)`
      for macOS, and no-op-plus-`Err("unsupported")` stubs elsewhere, with one
      debug-only command to drive it.
- [ ] No existing behavior changed in release builds.

## Before you start

- **Current destroy path** (read these first):
  - `crates/er-desktop/src/browser_webview.rs:257` — `fn park_tab_webview(wv: &tauri::Webview)`:
    moves the child to `-10000,-10000`, shrinks to 1×1, calls `hide()`.
  - `crates/er-desktop/src/browser_webview.rs:284` — `pub fn reset_all_tab_webviews(…)`:
    closes every child.
  - `crates/er-desktop/src/browser_webview.rs:371` — `#[tauri::command] browser_suspend_for_overlay`:
    calls `reset_all_tab_webviews` then refocuses the main window. Its doc comment
    states the reason: "`hide()` alone is not always enough on macOS — destroyed
    webviews cannot intercept clicks."
  - `desktop-ui/src/lib/stores/overlay.svelte.ts` — `acquire()` calls
    `prepareOverlayFocus()`, which invokes `browser_suspend_for_overlay`.
  - `desktop-ui/src/lib/components/ui/ModalShell.svelte:49` — every modal calls
    `overlay.acquire()`.
- **Native access.** Tauri's `Webview::with_webview(|platform| …)` gives the
  platform object on macOS: `platform.inner()` is the `WKWebView` pointer,
  `platform.ns_window()` the window. The repo already depends on `objc2 = "0.6"`
  for macOS (`crates/er-desktop/Cargo.toml`, target section). Wry pulls
  `objc2-web-kit 0.3.2` (see `Cargo.lock`); if you add it to `er-desktop`, use the
  same `0.3` line so only one copy builds, and enable the per-class features you
  use (README, "Global gotchas", lists them).
- **Run the app:** `./scripts/tauri-dev.sh` from the repo root. Start the fixture
  server from T01 first.

## Scope

- **In scope:** new `crates/er-desktop/src/browser_native.rs`, its `mod` line in
  `crates/er-desktop/src/lib.rs`, one debug-only probe command registered in
  `crates/er-desktop/src/main.rs` inside the `invoke_handler` list
  (`main.rs` around line 2069, next to `browser_webview::browser_ensure`),
  `Cargo.toml` additions for `objc2-web-kit`/`objc2-app-kit` on macOS only, the proof log.
- **Out of scope:** `overlay.svelte.ts`, `ModalShell.svelte`, the existing
  suspend path. Leave the product's destroy-on-modal behavior as it is.

## Steps

### Step 1 — Write the candidate mechanism

- **Goal:** `park` and `unpark` for one child webview.
- **Why / invariant:** the same `WKWebView` instance must survive (same document,
  same `WKWebsiteDataStore`, same back-forward list). Anything that closes or
  recreates the webview fails the proof by definition.
- **Do this:** in `park`, on the main thread: record current frame; remove first
  responder from the WKWebView (make the window's content view first responder);
  `setHidden(true)`; move its frame off the visible content (keep a non-zero
  size, e.g. 1×1 at the bottom-right corner inside the window); and, as the
  strongest variant, `removeFromSuperview` while retaining it, re-adding it to the
  same superview in `unpark`. Implement the variants behind an enum so each can be
  tried: `HideOnly`, `HideAndResign`, `Detach`. `unpark` restores frame first,
  then visibility, then focus.
- **Watch out for:** every AppKit call must run on the main thread. Tauri commands
  can run on other threads; use `app.run_on_main_thread`. Detaching may break
  Wry's assumptions about the view hierarchy; if Wry later re-adds or resizes the
  view itself, record that.
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" cargo check -p er-desktop` → no errors.

### Step 2 — Add the debug probe command

- **Goal:** drive park/unpark from the app without modals.
- **Do this:** `#[cfg(debug_assertions)] #[tauri::command] fn browser_probe_park(app, tab_idx: usize, variant: String, park: bool)`,
  using `browser_webview::webview_label(tab_idx)` to find the child. Register it
  under `#[cfg(debug_assertions)]` only. Call it from the devtools console:
  `__TAURI_INTERNALS__.invoke('browser_probe_park', {tabIdx:0, variant:'Detach', park:true})`.
- **Verify:** the call returns without error in a dev build.

### Step 3 — Run the scenarios

For each variant, in a real window, with the browser open on
`http://localhost:5173/form.html` after typing text and scrolling halfway:

1. Park. Click and type where the page was. **Expect:** the shell under it gets the
   clicks and keys; the page gets none (its text does not change).
2. Open a real modal (any settings dialog) while parked. **Expect:** the modal
   receives clicks and keyboard input, including Escape.
3. Unpark. **Expect:** same `#doc-id`, typed text present, scroll position kept,
   Back still returns to the previous page.
4. Repeat park/unpark 50 times quickly (devtools loop). **Expect:** no crash, no
   focus theft, page still interactive.
5. With `dialog.html` open and its native `<dialog>` showing, park and unpark.
6. Switch the app to another window and back while parked.

Record each result, per variant, in the proof log. Note the macOS version.

### Step 4 — Decide

- **Result yes:** at least one variant passes all six scenarios repeatedly. Name
  it in the log. Delete the losing variants from the module.
- **Result no:** record which scenario fails for each variant. Leave the module in
  place but unused, and stop. T09 cannot start.

## Rollback

Remove `browser_native.rs`, its `mod` line, the probe command registration and
the Cargo additions.

## When to stop and ask

- Every variant fails any scenario.
- A variant passes but needs a timer or retry to work ("works after 100 ms").
  Record it as **no**; flaky parking is the bug we are removing.
- Wry or Tauri re-shows the view on its own after `park`.
