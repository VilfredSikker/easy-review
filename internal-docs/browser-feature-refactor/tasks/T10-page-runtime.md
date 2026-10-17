# T10 — Page runtime in the content world

Status: ready-for-agent
Depends on: T03 (content world proven "yes"), T08 (message envelope)
Merges with: T11, in one PR. T10 removes the in-page note editor and T11 adds its
replacement; merged alone, T10 would leave no way to save an annotation.
Size: 3–4 days
Risk: high (trust boundary; this code runs inside arbitrary web pages)

> **For the executor:** Do not start unless `proof-log.md` `## T03 content world`
> says **Result: yes**. Build exactly this, in order, on a branch that T11 then
> builds on. Open one PR for both tasks.

## In plain terms

- **The problem.** The annotation script lives in the page's own JavaScript, where
  the page can switch it off, call its channel to the app, and fake clicks. It
  also draws the note editor inside the page, and its saved element locations
  cannot find elements again inside shadow DOM or the right iframe.
- **The fix.** A new, smaller runtime that runs in a separate JavaScript world the
  page cannot see. It only reads the page, outlines what the reviewer points at,
  draws numbered markers, and reports geometry and element descriptions to the
  app. It ignores synthetic events. It never shows a text field.
- **TL;DR.** Rewrite the in-page script to run out of the page's reach and only
  point, outline and report.

## Definition of done

- [ ] New runtime source at `crates/er-desktop/src/page_runtime.js`, embedded with
      `include_str!` and installed through `browser_native::install_runtime`
      (T03) at document start, main frame only.
- [ ] Runtime ↔ host messages use the T08 envelope over the content-world handler;
      the Rust side passes `frameInfo.isMainFrame` into validation and rejects
      subframe and stale-document messages.
- [ ] Every input handler checks `event.isTrusted`.
- [ ] Picking: element (heuristic in Step 3), region drag ≥ 4 px, open shadow roots,
      same-origin iframes; cross-origin iframe and closed roots produce a region
      target with an explicit limitation.
- [ ] Hold Space suspends annotate mode; Escape steps back one level.
- [ ] Locators record frame chain, shadow-host chain, selector and fingerprint;
      resolution reports `resolved`, `missing` or `ambiguous`.
- [ ] Page UI (outline, region box, markers, cursor) lives in one closed shadow
      root on `<html>`, top layer via `popover="manual"`, re-attached if removed.
- [ ] Old injection (`initialization_script(FRAME_SCRIPT)`, `inject_frame_script`)
      is no longer used on any platform. On macOS the runtime is installed in the
      content world. On Windows and Linux the same `page_runtime.js` is installed
      with `initialization_script` (page world) and the browser reports the
      capability `annotation_input_isolated: false`; the host treats every
      message from those pages as untrusted, as the spec requires.
- [ ] `browser-remote.json` gets `"platforms": ["windows", "linux"]`, so macOS
      remote pages have no Tauri capability at all.
- [ ] The fixture checks in Step 7 pass.

## Before you start

- Read `reference/spec.md` sections "Page runtime and trust" and "Annotation
  identity and geometry", and `reference/chatgpt-report-preload.md` sections 3–5
  (ChatGPT's overlay, picking and anchors — a working model of the same runtime).
- **Current script:** `crates/er-desktop/src/frame_script.rs` (764 lines). Reusable
  logic to port, not copy verbatim:
  `pierceShadowAtPoint` (line 213), `deepElementFromPoint` (223),
  `deepQuerySelector` (241), `domContext` (142), `elementContext` (134),
  `cssPath` (77; lacks shadow-host chain — replace), hover box (284–303).
  The composer (`showComposer` 632, `submitComposer` 615) is **not** ported.
- **Native install:** `browser_native::install_runtime` and `post_to_runtime` (T03).
- **Validation:** `crates/er-desktop/src/browser_messages.rs` (T08).

## Scope

- **In scope:** `page_runtime.js`, its install path in `browser_webview.rs`, the
  macOS message handler wiring into `browser_messages.rs`, deletion of the
  `browser-remote` capability for macOS, runtime tests.
- **Out of scope:** the editor (T11), storage (T12), capture (T13). The runtime
  must still render markers from data the host sends.

## Steps

### Step 1 — Skeleton and transport

- **Do this:** IIFE that installs once per document (guard in the runtime's own
  world). On start: post `Hello{href}`; receive the document token; then `Ready`.
  Expose `__erRuntime.receive(json)` **inside the runtime world** for host → runtime.
- **Why / invariant:** the runtime's globals exist only in its world. Never write
  to `window` properties the page could read except DOM nodes it owns.
- **Verify:** real app on `landing.html`: the backend logs one accepted `Hello`
  and one `Ready` per page load (T08 logs accepted kinds); in the page console,
  `typeof __erRuntime` is `"undefined"`.

### Step 2 — Overlay

- **Do this:** host element appended to `document.documentElement`, closed shadow
  root, explicit style reset, `position:fixed; inset:0; pointer-events:none`.
  Interaction layer is a `div popover="manual"` shown with `showPopover()` while
  annotate mode is on, re-raised on the page's `toggle` and `fullscreenchange`
  events. A `MutationObserver` re-appends the host if the page removes it.
  Hover outline `2px solid #0069FB`, 3% fill; markers numbered (the number comes
  from the host), 26 px teardrop. While annotate mode is on, hide the page cursor
  with an injected style and draw an overlay cursor.
- **Watch out for:** to hit-test under the overlay, set the overlay's
  `pointer-events` to `none`, call `elementFromPoint`, restore in `finally`.
- **Verify:** `dialog.html` with the native dialog open: hovering in annotate mode
  outlines elements inside the dialog (the overlay is above it). In the page
  console, `document.documentElement.lastElementChild.shadowRoot` is `null`
  (closed root). Removing the host node from the console brings it back within
  one frame.

### Step 3 — Picking

- **Do this:** walk up from the hit element; return the first `a`, `button`,
  `input`, `textarea`, `select`, `label`, `img`, any `[role]`, or an element with
  text whose box is ≥ 24×16 CSS px; otherwise the outermost ancestor ≥ 48×24;
  `html`/`body` only when hit directly. Descend open shadow roots and same-origin
  iframes. A drag ≥ 4 px makes a region target. Cross-origin iframes and closed
  roots: region target with `limitation: "cross-origin-frame" | "closed-shadow-root"`.
- **Input:** capture-phase listeners for pointer, mouse, click, contextmenu and
  wheel stop propagation to the page while annotating; every handler returns early
  unless `event.isTrusted`. Wheel events scroll the scrollable ancestor under the
  cursor. Hold Space (outside editable fields) suspends; release resumes.
  Escape: clear selection → cancel drag → tell host `ExitAnnotate`.
- **Verify:** on `landing.html`, clicking text inside a link selects the link, not
  the inner span; on `canvas.html`, a drag selects a region; on `hostile.html`,
  the synthetic clicks select nothing (backend logs no `Select`).

### Step 4 — Locator and fingerprint

- **Do this:** locator `{ frames: [selector…], shadowHosts: [[selector…] per frame level], selector, fingerprint: { role, name (≤ 80 chars), text (≤ 80 chars), tag } }`.
  Selector: tag, `#id` if unique in its root, up to two stable classes,
  `:nth-of-type`. Resolution follows the chain; exactly one match with a compatible
  fingerprint → `resolved`; zero → `missing`; more than one or conflicting
  fingerprint → `ambiguous`. Never pick the first match silently.
- **DOM context (bounded, ≤ 24 KiB):** tag, role, accessible name, short text,
  selected safe attributes. **Exclude** values of password and hidden inputs,
  attributes whose name contains `token`, `secret`, `password`, `auth`, `key`, and
  any live form value.
- **Verify:** select `button.save` in each frame of `frames.html`; the two
  locators differ in `frames`; on `password.html` the reported DOM context does
  not contain `secret-token` or `abc`.

### Step 5 — Geometry and markers

- **Do this:** report viewport rect, viewport size, scroll offsets, device scale
  and zoom with each selection. Re-resolve saved targets on scroll, resize and
  filtered mutations, coalesced to one per animation frame; send
  `AnchorUpdate` only when status or rect changed meaningfully (≥ 1 px).
- **Verify:** on `nested-scroll.html`, scrolling the container for 2 s produces at
  most ~120 `AnchorUpdate` messages (one per frame) and the marker stays on the
  item; resizing the window across 600 px on `responsive.html` keeps status
  `resolved`.

### Step 6 — Install the runtime and retire the old script

- **Do this:** in `ensure_tab_webview`: on macOS call
  `browser_native::install_runtime(&wv, PAGE_RUNTIME)`; elsewhere use
  `.initialization_script(PAGE_RUNTIME)` and have the runtime post through Tauri
  IPC (`browser_host_message`) when `window.webkit?.messageHandlers?.erAnnotation`
  is absent. Delete `inject_frame_script` and stop using `FRAME_SCRIPT`. Add
  `"platforms": ["windows", "linux"]` to `crates/er-desktop/capabilities/browser-remote.json`
  (Tauri 2 capabilities support a `platforms` list; verified in
  `tauri-utils-2.9.1/src/acl/capability.rs`).
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" cargo check -p er-desktop`
  passes; in the macOS app, the devtools console of a reviewed page shows
  `window.__TAURI_INTERNALS__` is `undefined` or has no permitted commands
  (an `invoke('browser_host_message', …)` call is rejected).
- **If it fails:** if macOS pages still get Tauri IPC, the capability's
  `platforms` value is wrong — check the `Target` enum spelling in that file.

### Step 7 — Checks

- **Runtime unit tests:** run the picking, locator and fingerprint functions in a
  real browser engine (Bun with happy-dom is not enough for layout; use the
  repo's Playwright or Storybook setup if present, otherwise a fixture page loaded
  in the app with assertions logged to the console). Cover: `frames.html` (two
  `button.save` → each resolves to its own frame), `shadow.html` (open root target
  re-resolves after reload; closed root → region with limitation),
  `nested-scroll.html` (target followed during scroll), `password.html` (no
  secret values in DOM context).
- **Hostile page:** `hostile.html` produces no annotation, no selection, no
  message accepted by the host; drops are logged with reason.
- **Rust:** `cargo test -p er-desktop browser_messages` still passes, plus a test
  that a subframe-flagged message is rejected.

## Rollback

Revert; macOS returns to the old injected script.

## When to stop and ask

- The page can observe the runtime's globals or handler.
- A required picking behavior needs code in the page world.
