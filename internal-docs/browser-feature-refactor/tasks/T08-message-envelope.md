# T08 — Message envelope and sender validation

Status: ready-for-agent
Depends on: T06 (browser registry with labels and generations)
Size: 1–2 days
Risk: high (trust boundary: messages from arbitrary web pages)

> **For the executor:** Build exactly this, in order. Preserve each invariant; if
> the code differs from what is cited, stop and ask.

## In plain terms

- **The problem.** When a page's annotation script sends the app a message, the
  backend broadcasts the raw message to the whole app with no record of which
  browser sent it, and the UI applies it to whichever tab is visible. A message
  from a hidden or old page can change the visible one.
- **The fix.** Every page message is checked in Rust: which browser sent it (from
  the native sender, never from the message text), whether it is from that
  browser's current page, whether it is a known message type, and whether its
  numbers and strings are within limits. Valid messages go only to that browser's
  subscribers; everything else is dropped and logged.
- **TL;DR.** Know who sent each page message, and throw away anything malformed
  or stale.

## Definition of done

- [ ] `crates/er-desktop/src/browser_messages.rs` defines a versioned envelope and a
      typed enum of runtime messages, with bounded decoding.
- [ ] `browser_host_message` no longer emits the raw payload. It resolves the
      sender's tab from the invoking webview label via `BrowserRegistry::tab_for_label`
      (T06), validates, and emits `browser://runtime` with `{tab_id, generation,
      document_id, message}`.
- [ ] Messages whose generation or document is not current are dropped.
- [ ] The frontend decodes with a matching validator and ignores events for other
      tabs or stale generations.
- [ ] The development iframe path (`onWindowMessage`) is disabled whenever the
      native browser is active.
- [ ] Negative tests listed in Step 5 pass.

## Before you start

- **Current relay:** `crates/er-desktop/src/browser_webview.rs:466-470`:
  ```rust
  #[tauri::command]
  pub fn browser_host_message(app: AppHandle, payload: serde_json::Value) -> Result<(), String> {
      app.emit(BROWSER_MESSAGE_EVENT, payload)
          .map_err(|e| e.to_string())
  }
  ```
  `BROWSER_MESSAGE_EVENT` comes from `crates/er-desktop/src/frame_script.rs`.
- **Getting the sender:** a Tauri command can take `webview: tauri::Webview` as a
  parameter; `webview.label()` is the invoking webview. Use that, never a field in
  the payload.
- **Frontend consumers:** `desktop-ui/src/lib/stores/browserHost.ts`
  (`listenBrowserMessages`), `desktop-ui/src/lib/components/BrowserView.svelte`
  (`handleBrowserPayload` line 595, `onWindowMessage` line 602, the listener set up
  in `onMount` around line 646).
- **Message kinds in use today:** grep `__er_` in `frame_script.rs` and
  `BrowserView.svelte` (for example `__er_ready`, `__er_location`,
  `__er_composer_submit`, `__er_query_rect_result`, `__er_reanchor_result`,
  `__er_annotate_mode_ack`, shortcut forwarding). T10 replaces the runtime; this
  task defines the envelope both the current script and the new runtime use, so
  model the enum on the T10 message list too (`reference/spec.md`, "Page runtime
  and trust"; ChatGPT's list in `reference/chatgpt-report-preload.md` section 1 is
  a useful model).
- **Document identity:** T07's `BrowserNavState.document_id`. The page learns its
  document token through the bootstrap handshake (spec: "Document-start messages
  begin as a bootstrap handshake").

## Scope

- **In scope:** `browser_messages.rs`, `browser_host_message`, the frontend
  decoder and listener, tests.
- **Out of scope:** what the UI does with each message (T10/T11), the content-world
  transport (T10 plugs into this validator).

## Steps

### Step 1 — Define the envelope

- **Do this:** `RuntimeEnvelope { v: u8 /* = 1 */, kind: String, doc: String /* document token */, req: Option<String>, payload: serde_json::Value }`
  and `enum RuntimeMessage { Hello{href}, Ready, Location{href}, Hover{rect}, Select{target}, RegionSelect{rect}, AnchorUpdate{id, rect, status}, CaptureReady{req, target_rect, marker, skip}, … }`.
  Decode `payload` into the variant for `kind`; unknown `kind` → reject.
- **Why / invariant:** the page never supplies browser or tab identity, and never
  supplies note text that will be saved (T11 moves the editor to the host). The
  one exception is temporary: keep today's `__er_composer_submit` working as a
  `LegacyComposerSubmit` variant; T11 deletes it. Do not cite the task in a code
  comment — `just comments-check` rejects comments that name plan steps. Any other
  variant that carries reviewer text is a design error — stop and ask.

### Step 2 — Bounds

- **Do this:** named constants: envelope ≤ 256 KiB serialized, any string ≤ 4 KiB
  unless the field says otherwise, DOM context ≤ 24 KiB, collections ≤ 256 items,
  coordinates finite and within ±1e6, sizes ≥ 0. Reject on violation with a reason.

### Step 3 — Validate the sender and route

- **Do this:** `browser_host_message(webview: tauri::Webview, app, payload)`:
  label → `(tab_id, generation)` via the registry; unknown label → drop. Document
  token must equal the browser's current document token (T07 state); otherwise
  drop as stale, except the bootstrap `Hello`, which the backend answers with the
  current token. Emit `browser://runtime` with the validated message. Log drops
  with reason, tab ID and kind (never the payload body).
- **Watch out for:** the main shell webview must not be able to call this command
  as a browser; reject labels not in the registry.

### Step 4 — Frontend decoder

- **Do this:** a pure module `desktop-ui/src/lib/stores/browserMessages.ts` that
  validates `browser://runtime` events (shape, kind, tab, generation) and returns a
  typed message or `null`. BrowserView handles only messages for its tab and
  current generation. Remove the `listen(BROWSER_MESSAGE_EVENT, …)` raw listener.
  Make `onWindowMessage` a no-op unless the explicit dev proxy is active, and then
  check `event.source === iframeEl.contentWindow`.

### Step 5 — Tests

- **Rust:** unknown kind; oversized envelope; NaN coordinate; string over limit;
  unknown label; label of a closed tab; old generation; old document token;
  `Hello` from a new document gets a token; payload claiming a `tab_id` is ignored.
  Run `cargo test -p er-desktop browser_messages` (through `locked.sh`).
- **TS:** the same shapes against the decoder:
  `cd desktop-ui && bun run test src/lib/stores/browserMessages.test.ts`.
- **Real app:** with two tabs on different fixture pages, a hidden tab finishing a
  load does not change the visible tab's address or state.

## Rollback

Revert the commits; the raw relay returns.

## When to stop and ask

- A current UI feature depends on a message kind you cannot type without carrying
  reviewer text from the page.
- `tauri::Webview` is not available as a command parameter in the locked version.
