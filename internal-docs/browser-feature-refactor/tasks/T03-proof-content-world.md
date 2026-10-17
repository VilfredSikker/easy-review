# T03 — Proof: annotation runtime in a WebKit content world

Status: ready-for-human (needs native checks in the app window)
Depends on: T01
Size: 1–2 days
Risk: high (this is the trust boundary between web pages and the app)

> **For the executor:** Experiment with a yes/no outcome. Record it in
> `internal-docs/browser-feature-refactor/proof-log.md` under `## T03 content world`
> and stop. T10 builds the real runtime on top of what you prove here.

## In plain terms

- **The problem.** The annotation script is injected into the page's own
  JavaScript. The page can call the same app function the script uses and save a
  note with any text it likes, which later reaches an AI agent.
- **The question.** Can we run our script in a separate JavaScript "world" that
  shares the page's DOM but not its variables, with a message channel that only
  our world can see — without breaking Tauri's own messaging?
- **TL;DR.** Prove a page cannot talk to our annotation channel.

## Definition of done

- [ ] `proof-log.md` `## T03 content world` records environment and Result
      **yes** or **no**, with the outcome of every scenario in Step 4.
- [ ] If yes: `crates/er-desktop/src/browser_native.rs` (created in T02, or create
      it now) has `install_runtime(&tauri::Webview, script: &str)` and
      `post_to_runtime(&tauri::Webview, json: &str)`, and a channel that delivers
      runtime messages to Rust with the sending frame's `isMainFrame` flag and URL.
- [ ] Wry's normal IPC (the shell app and every existing `invoke`) still works.

## Before you start

- **Today's injection** (read these):
  - `crates/er-desktop/src/frame_script.rs:6` — `pub const FRAME_SCRIPT: &str`,
    starting `(function(){ if(window.__er_injected)return; …`. It reaches the host
    through `window.__TAURI_INTERNALS__.invoke('browser_host_message', …)`.
  - `crates/er-desktop/src/browser_webview.rs:174-176` — `WebviewBuilder::new(label, WebviewUrl::External(parsed)).initialization_script(FRAME_SCRIPT)`.
  - `crates/er-desktop/src/browser_webview.rs:133-137` — `inject_frame_script` evals
    the script again on each load. Its comment claims initialization scripts run
    only at creation; the Tauri docs say they run on every new document. Both run
    in the **page** world.
  - `crates/er-desktop/src/browser_webview.rs:467` — `browser_host_message` emits the
    raw payload app-wide.
- **WebKit pieces you need** (macOS 11+), reached through
  `tauri::Webview::with_webview(|p| p.inner())` → `WKWebView`:
  - `WKContentWorld::worldWithName("er-annotation")`.
  - `WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld`
    with `AtDocumentStart` and `forMainFrameOnly = true`.
  - `webView.configuration.userContentController`:
    `addUserScript`, and `addScriptMessageHandler_contentWorld_name` with a handler
    object implementing `WKScriptMessageHandler`. A handler registered for a
    content world is visible as `window.webkit.messageHandlers.<name>` **only** in
    that world.
  - Host → runtime: `evaluateJavaScript_inFrame_inContentWorld_completionHandler`
    (or `callAsyncJavaScript…inContentWorld`).
  - Each `WKScriptMessage` has `frameInfo` (`isMainFrame`, `request.URL`) and
    `world`.
- **Bindings:** `objc2-web-kit 0.3` (the version Wry already uses; check
  `Cargo.lock`, entry `objc2-web-kit`). Enable its features `WKContentWorld`,
  `WKUserScript`, `WKUserContentController`, `WKScriptMessage`,
  `WKScriptMessageHandler`, `WKFrameInfo`, `WKWebViewConfiguration`, `WKWebView`.
  Define the handler class with objc2's `define_class!`.
- **Do not touch** Wry's own handler named `ipc` in the page world. Use a distinct
  handler name, e.g. `erAnnotation`.

## Scope

- **In scope:** `browser_native.rs`, the macOS target dependencies in
  `crates/er-desktop/Cargo.toml`, a debug-only probe command, the proof log.
- **Out of scope:** `frame_script.rs` and the product injection path. Leave them.

## Steps

### Step 1 — Install a probe runtime in a named world

- **Goal:** a tiny script in world `er-annotation` that, at document start, posts
  `{type:"hello", href: location.href, docId: crypto.randomUUID()}` and listens for
  clicks (checking `event.isTrusted`) and posts `{type:"click", trusted, tag}`.
- **Why / invariant:** the script must be able to read the page's DOM (shared) but
  the page must not see the script's globals or its message handler. If the page
  can see either, the proof fails.
- **Do this:** implement `install_runtime` on the main thread via
  `app.run_on_main_thread`. Add it to a test child created by a debug-only probe
  command `browser_probe_world(app, tab_idx)` right after the child exists.
- **Verify:** `cargo check -p er-desktop` (through `locked.sh`) passes.

### Step 2 — Receive messages in Rust

- **Goal:** the handler forwards `(body_json, is_main_frame, frame_url, world_name)`
  to a Rust callback that, for the probe, prints with `eprintln!` and emits a
  debug event.
- **Watch out for:** the message body arrives as an `NSObject`; ask the runtime to
  post a JSON **string** and parse it in Rust. Keep the handler object alive for
  the webview's lifetime (store it; a dropped handler stops delivery silently).

### Step 3 — Send to the runtime

- **Goal:** `post_to_runtime` evaluates `window.__erRuntime.receive(<json>)` in the
  named world.
- **Verify:** a round trip "ping → pong" prints in the terminal.

### Step 4 — Run the scenarios

Open each fixture in the probe child and record results:

1. `landing.html`: `hello` arrives once per document, with `is_main_frame = true`.
   Navigate to another page: a new `hello` with a new `docId` arrives (document-start
   injection runs per document).
2. `hostile.html`: none of its attempts produce a message on `erAnnotation`;
   `window.webkit.messageHandlers.erAnnotation` is `undefined` in the page console;
   setting `window.__er_injected` has no effect on the runtime.
3. Synthetic clicks from `hostile.html` arrive (if at all) with `trusted=false`;
   real clicks with `trusted=true`.
4. `frames.html`: no message arrives from iframes (`forMainFrameOnly`), or if one
   does, `is_main_frame = false` identifies it.
5. The shell app still works: open settings, run any command that uses `invoke`.
6. Reload 20 times: one `hello` per load, no duplicates.

### Step 5 — Decide

- **Yes:** scenarios 1–6 pass. Record it.
- **No:** record which failed. Note whether "editor in the host, page sends only
  geometry" (the spec's fallback) would still be safe given what failed.

## Rollback

Remove the probe command and the world code from `browser_native.rs`.

## When to stop and ask

- Adding the handler breaks Wry IPC or Tauri `invoke` in any webview.
- The page can see `erAnnotation` or any global the runtime defines.
- `frameInfo` is unavailable or wrong for main-frame messages.
