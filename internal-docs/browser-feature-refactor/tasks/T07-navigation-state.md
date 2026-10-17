# T07 — Backend navigation state and address rules

Status: ready-for-agent
Depends on: T06 (browsers keyed by tab ID), T05 (navigation observation proven)
Size: 2–3 days
Risk: ordinary (user-visible browser behavior; no stored data)

> **For the executor:** Build exactly this, in order. Each step's invariant is what
> must hold; if the code differs from what is cited, stop and ask.

## In plain terms

- **The problem.** The address bar and loading spinner are driven by what the app
  *asked* to load and by messages from the page script. Switching between local
  servers on different ports can be skipped as "same page"; a redirect can be
  ignored; there is no Back, Forward or Stop; a failed load is invisible; and if
  the native browser fails, the app silently switches to a proxy that behaves
  differently.
- **The fix.** The backend watches the real browser (proven in T05) and owns one
  navigation state per browser: requested URL, committed URL, title, loading,
  Back/Forward availability and last failure. The UI only displays it and sends
  commands.
- **TL;DR.** The browser tells the app where it is; the app stops guessing.

## Definition of done

- [ ] `localhost:5173/x` → `localhost:3000/x` navigates (fixture `landing.html`).
- [ ] Redirects (`/redirect/chain`, `/redirect/origin`) end with the final URL
      shown and loading off.
- [ ] pushState / replaceState / hash routes update the address without reloading
      (`spa.html`, `#doc-id` unchanged).
- [ ] Back, Forward, Reload, Stop work and are disabled when unavailable.
- [ ] A refused port or 404 shows a failure state with Retry and Open externally.
- [ ] Switching review tabs never navigates a page.
- [ ] `window.open` and `target=_blank` open the external browser; popups without
      user activation are denied.
- [ ] The automatic proxy fallback is gone; the proxy remains reachable only by an
      explicit developer setting.
- [ ] Unit tests below pass.

## Before you start

- Read `reference/spec.md` sections "User-visible flow" and "Navigation, origins,
  and sessions".
- **Rust today** (`crates/er-desktop/src/browser_webview.rs`):
  - `:56` `parse_nav_url` — prefixes `http://` to anything without a scheme.
  - `:81` `browser_urls_equivalent` — compares scheme, host, path and query;
    **drops the port and the fragment**. Used at `:211` to skip navigation.
  - `:327` `on_tab_selected` — navigates the child to `tab.browser_url` on every
    tab switch when URLs differ.
  - `:434` `browser_navigate`, `:478` `browser_reload` commands.
- **Frontend today:**
  - `desktop-ui/src/lib/stores/browserUrl.ts` — `canonicalizeBrowserUrl` (keeps
    the port via `u.host`, drops the hash), `sameBrowserUrl`, `pageKey`,
    `toProxyUrl` / `fromProxyUrl`.
  - `desktop-ui/src/lib/components/BrowserView.svelte` — `openNativeBrowser`
    (line 76) and `navigateBrowser` (line 91) set `useProxyFallback = true` on any
    error; `go()` (line 339); `refresh()` (line 353); `applyPageLocation` (line 374)
    ignores a location that differs from `pendingNavigationUrl`.
  - `desktop-ui/src/lib/stores/browser.svelte.ts` — the browser store.
- **External open:** `crates/er-desktop/src/commands.rs:1567` —
  `open_url_in_browser(url)` calls `open_external_url`.
- **Native observation:** the `observe`, `go_back`, `go_forward`, `can_go_*`, `stop`
  functions in `crates/er-desktop/src/browser_native.rs` from T05. The ADR from T05
  says which failure signals are reliable.
- **Push model:** emit events; do not poll (ADR 0011).

## Scope

- **In scope:** a new `crates/er-desktop/src/browser_nav.rs` (state + address
  rules), navigation commands, `browser_webview.rs` wiring, the browser store and
  the toolbar in `BrowserView.svelte`, `browserUrl.ts`, tests.
- **Out of scope:** annotation code, modal handling (T09), message envelope (T08).

## Steps

### Step 1 — Address rules in Rust

- **Goal:** one function `normalize_address(input: &str) -> Result<Url, AddressError>`.
- **Why / invariant:** Rust is the single source of truth; the frontend sends what
  the user typed.
- **Rules:** trim; blank → `Err(Blank)` (leave the page alone); explicit `http://`
  or `https://` kept; any other explicit scheme → `Err(UnsupportedScheme)`;
  bare `localhost`, `*.localhost`, `127.0.0.1`, `0.0.0.0`, `[::1]` (with or without
  port and path) → `http://`; other bare hosts → `https://`. No search fallback.
- **Verify:** table-driven unit tests covering every rule, including
  `localhost:3000/a?b#c`, `app.localhost`, `[::1]:8080`, `example.com`, `ftp://x`,
  `javascript:alert(1)` (rejected), `"  "` (blank).

### Step 2 — URL equality

- **Do this:** replace `browser_urls_equivalent` with `same_navigation_target(a, b)`
  comparing scheme, host, **effective port**, path, query and **fragment**.
  Annotation page matching is a separate function (`pageKey` in TS today) and is
  not changed here.
- **Verify:** tests: ports 5173 vs 3000 differ; `/a#x` vs `/a#y` differ;
  `http://LOCALHOST:80/` equals `http://localhost/`.

### Step 3 — Navigation state

- **Do this:** `BrowserNavState { browser_id, generation, sequence: u64, navigation_id: Option<u64>, document_id: u64, requested_url: Option<String>, committed_url: String, title: String, loading: bool, can_go_back: bool, can_go_forward: bool, failure: Option<NavFailure> }`.
  Update it from native observation callbacks. Rules:
  - A new navigate command creates a new `navigation_id`; results from older
    attempts are ignored.
  - A committed URL change that loads a new document increments `document_id`;
    a same-document change (pushState, hash) does not.
  - While loading, the address shown is the last committed URL; `requested_url`
    is shown only as a pending hint.
  - Every change increments `sequence` and emits `browser://state` with the whole
    state and `tab_id`.
- **Watch out for:** the frontend may receive events out of order; it must drop any
  state whose `(generation, sequence)` is not newer than what it has.
- **Verify:** unit tests drive the state with a fake event stream: redirect chain,
  same-document change, stale navigation result, failure then retry.

### Step 4 — Commands

- **Do this:** `browser_navigate(tab_idx, input)` uses `normalize_address`;
  add `browser_go_back`, `browser_go_forward`, `browser_stop`; keep
  `browser_reload`. Register them in `crates/er-desktop/src/main.rs` beside the
  existing `browser_webview::` commands (around line 2069). Submitting the
  committed URL reloads.
- **Why / invariant:** Back/Forward use native history only; never keep a URL
  array or evaluate `history.back()`.

### Step 5 — Tab switch never navigates

- **Do this:** `on_tab_selected` shows the tab's existing child; it creates one
  (and navigates to the stored URL) only when the tab has no child yet.
- **Verify:** real app: tab A on `spa.html`, press pushState (address changes,
  same `#doc-id`); switch to tab B and back. **Expect:** tab A still shows the
  pushed route and the same `#doc-id` (no reload to the stored URL).
- **If it fails:** a reload means something still navigates on select — grep
  `browser_webview.rs` for `navigate(` and `finish_browser_navigate` callers.

### Step 6 — Popup policy

- **Do this:** on the child's `WebviewBuilder`, `on_new_window(|url, _| { open_external(url); NewWindowResponse::Deny })`
  for user-activated requests. If the native hook cannot tell activation, deny
  everything and open externally only for link clicks reported by the hook; record
  which in the code comment. Show a short notice in the browser pane: "Opened in
  your browser" with an "Open page externally" action (`open_url_in_browser`
  with the committed URL).
- **Verify:** real app on `popup.html`: the button and the `_blank` link each open
  the default browser once and show the notice; the timed script popup opens
  nothing. `cargo check -p er-desktop` passes.
- **If it fails:** if the timed popup also opens externally, the hook cannot see
  activation — switch to deny-all-except-link-clicks and record it in the code
  comment and the PR.

### Step 7 — Frontend reads state; remove automatic fallback

- **Do this:** the browser store subscribes to `browser://state`, keeps the newest
  per `tab_id`, and exposes it. The toolbar shows Back, Forward, Reload or Stop,
  address field, Open externally. Address edits stay local while the field has
  focus; a committed change does not overwrite text being edited. Delete
  `applyPageLocation`'s pending-URL rejection and the `useProxyFallback = true`
  assignments. On failure show Retry and Open externally. Keep the proxy reachable
  only when a developer flag is set (add it to the existing config only if one
  already exists for dev features; otherwise stop and ask).
- **Verify:** `cd desktop-ui && bun run test src/lib/stores/browser.svelte.test.ts`
  and `bun run check` (through `locked.sh`). Then the real-app checklist in the
  definition of done, with the fixture server.

## Rollback

Revert the task's commits.

## When to stop and ask

- T05 recorded that failures cannot be detected reliably.
- The native hook cannot distinguish user-activated popups and denying all would
  break a common flow.
- Removing the fallback leaves no way to show any page for a class of URLs.
