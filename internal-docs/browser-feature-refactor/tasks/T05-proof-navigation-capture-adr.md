# T05 — Proof: navigation events, page snapshot, data store; write the ADR

Status: ready-for-human (needs native checks)
Depends on: T02, T03, T04 (the ADR records all four proofs)
Size: 1–2 days
Risk: high (every later task builds on the mechanism chosen here)

> **For the executor:** Answer the three questions below with code you can show
> working, record results under `## T05 navigation, capture, data store` in
> `internal-docs/browser-feature-refactor/proof-log.md`, then write the ADR.

## In plain terms

- **The problem.** The app guesses what the browser is doing from what it asked
  for and from messages the page script sends. It cannot tell "redirected" from
  "failed", does not know if Back is possible, and cannot take a picture of the
  page itself.
- **The questions.**
  1. Can the backend observe navigation directly from WebKit: URL changes, loading
     start/finish, failure, title, and Back/Forward availability — without
     replacing the delegates Wry relies on?
  2. Can it take a snapshot image of exactly the page (with our outline drawn in
     it) at the right pixel scale?
  3. Do cookies survive an app restart, and do all review pages share them?
- **TL;DR.** Prove the backend can see and photograph the page, then write down
  the native design in an ADR.

## Definition of done

- [ ] Proof log records Result yes/no per question with observations.
- [ ] `browser_native.rs` has, for macOS: `observe(&tauri::Webview, callback)`
      (key-value observation on the WKWebView), `go_back`, `go_forward`,
      `can_go_back`, `can_go_forward`, `stop`, and `snapshot(&tauri::Webview) -> PNG bytes`.
- [ ] A new ADR in `docs/adr/` (next number after `0043-in-app-updates.md`, i.e.
      `0044-…`) records the native browser adapter: parking variant (T02), content
      world (T03), editor surface (T04), navigation observation and snapshot
      (this task), and what other platforms report as unsupported.
- [ ] `just docs-check` passes.

## Before you start

- **What Tauri gives you** (locked version 2.12.1; source under
  `~/.cargo/registry/src/*/tauri-2.12.1/src/webview/mod.rs`):
  - `WebviewBuilder::on_page_load` — only `PageLoadEvent::Started` and `Finished`.
    Used today at `crates/er-desktop/src/browser_webview.rs:177-184`.
  - `WebviewBuilder::on_navigation(|url| bool)` — allow/deny top-level navigation.
  - `WebviewBuilder::on_new_window(|url, features| NewWindowResponse)` — includes
    `NewWindowResponse::Deny`; needed for the popup policy in T07.
  - `WebviewBuilder::on_document_title_changed`.
  - `Webview::navigate`, `Webview::reload`, `Webview::url`, `Webview::with_webview`.
  - No Back/Forward/Stop/history API, no failure event.
- **Constraint:** Wry owns the `WKNavigationDelegate` and `WKUIDelegate`. Do not
  replace them. Key-value observation of `WKWebView` properties (`URL`, `title`,
  `loading`, `canGoBack`, `canGoForward`, `estimatedProgress`) does not touch the
  delegates — try it first.
- **Snapshot:** `WKWebView.takeSnapshotWithConfiguration:completionHandler:` with a
  `WKSnapshotConfiguration` whose `rect` is the visible bounds; the result is an
  `NSImage`. Convert to PNG (via `NSBitmapImageRep`, `objc2-app-kit`).
- **Features:** enable `objc2-web-kit` features `WKSnapshotConfiguration` and
  `WKBackForwardList` in addition to those T03 added (README, "Global gotchas").

## Scope

- **In scope:** `browser_native.rs`, macOS Cargo target deps, debug-only probe
  commands, the proof log, the ADR.
- **Out of scope:** BrowserView, engine state, any product path.

## Steps

### Step 1 — Observe navigation

- **Do this:** register KVO observers (objc2 `define_class!` observer object, kept
  alive with the webview) for the properties above. Forward each change as
  `(property, value)` to a Rust callback; for the probe, `eprintln!` with a
  timestamp. Combine with Tauri's `on_page_load` and `on_navigation` callbacks.
- **Scenarios** (fixture server from T01; record the observed event sequence for each):
  1. `landing.html` → `/redirect/chain` → ends at `/landing.html`. Final `URL`
     observed; loading settles false.
  2. `/redirect/origin` → final URL on `127.0.0.1`.
  3. `spa.html` pushState / replaceState / hash: `URL` changes, no new document
     (`#doc-id` unchanged), `loading` stays false.
  4. `/status/404` and a refused port (`http://localhost:5999/`): what tells you
     it failed? Record exactly which signals differ from success. If nothing
     reliable distinguishes failure, Result for Q1 is **partial**: record it.
  5. Back and Forward availability matches the page history after 3 navigations.
  6. `/slow?ms=5000`, then Stop after 1 s: the old document stays, `loading` false.
  7. Navigate `localhost:5173/landing.html` → `localhost:3000/landing.html`: a real
     navigation happens (the port differs).

### Step 2 — Snapshot

- **Do this:** `snapshot()` returns PNG bytes of the visible page.
- **Scenarios:** on a Retina display and at window zoom 100% and 125%:
  1. Image dimensions equal visible CSS size × device scale.
  2. An element outlined by the T03 probe runtime (draw a 2px blue border in the
     content world) appears in the image at the expected position (± 2 px).
  3. The app shell and any T04 editor surface are **not** in the image.
  4. Snapshot while the page is parked (T02 variant): record whether it works.

### Step 3 — Data store

- **Scenarios:** visit `/set-cookie`, quit the app, relaunch, open `/read-cookie`
  in the same tab and in a second review tab. Record whether the cookie is
  present in both. Check `WKWebsiteDataStore` is the default persistent store
  (not `nonPersistentDataStore`).

### Step 4 — Write the ADR

- **Do this:** follow the format of an existing ADR (read
  `docs/adr/0043-in-app-updates.md` and `docs/agents/writing-docs.md`). Context:
  why native, why not destroy on modal, why a content world. Decision: the
  mechanisms that passed. Consequences: what other platforms lose; what is
  unproven (for example failure attribution if Step 1 scenario 4 was partial).
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" just docs-check` → `docs-check: ok`.

## Rollback

Delete the probe commands. Keep the ADR only if it records the decision accurately.

## When to stop and ask

- KVO requires replacing a Wry delegate.
- No signal distinguishes a failed load from a finished one (record it; the
  owner decides whether T07 shows failures via a fallback heuristic).
- Snapshot omits content-world drawing or includes the shell.
