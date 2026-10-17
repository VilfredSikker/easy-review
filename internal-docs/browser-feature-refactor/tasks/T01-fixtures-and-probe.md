# T01 — Fixture pages and native probe harness

Status: ready-for-agent
Depends on: nothing
Size: 1–2 days
Risk: ordinary (test-only code)

> **For the executor:** Build exactly what this file says, in order. Where a step
> says **Why / invariant**, that is what must stay true; if reality differs,
> preserve it and stop and ask. Do not touch product code in this task.

## In plain terms

- **The problem.** Every later task must prove browser behavior against real
  pages: redirects, single-page-app routes, iframes, shadow DOM, two ports, two
  origins. None of those pages exist yet, so every check would be ad hoc.
- **The fix.** A small local web server, started with one command, that serves a
  fixed set of test pages on two ports and two host names, plus a results log
  that the proof tasks fill in.
- **TL;DR.** Build the test pages once so every later check is repeatable.

## Definition of done

- [ ] `bun tests/fixtures/browser/serve.ts` serves the pages below on
      `http://localhost:5173` and `http://localhost:3000`, and both ports answer on
      `http://127.0.0.1:<port>` as a second origin.
- [ ] Each page in the table exists and does what its row says (verified with
      `curl` for server behavior and by opening it in Safari for DOM behavior).
- [ ] `internal-docs/browser-feature-refactor/proof-log.md` exists with one empty
      section per proof task (T02–T05).
- [ ] No file outside `tests/fixtures/browser/` and the proof log changed.

## Before you start

- Read [the README](../README.md) "Shared orientation".
- Bun is the repo's JavaScript runtime (`desktop-ui` uses it). `Bun.serve` needs no
  dependency.
- Existing fixtures live under `tests/fixtures/` (currently `tests/fixtures/arena/`).

## Scope

- **In scope:** `tests/fixtures/browser/**`, `internal-docs/browser-feature-refactor/proof-log.md`.
- **Out of scope:** any Rust or Svelte code; CI wiring (these pages are for manual
  and native checks).

## Steps

### Step 1 — Create the server

- **Goal:** one script serving every page on both ports.
- **Why / invariant:** later tasks need the *same* path on two ports (port must
  matter for navigation) and the same page on two origins (`localhost` vs
  `127.0.0.1`). Keep both properties.
- **Files:** create `tests/fixtures/browser/serve.ts` and `tests/fixtures/browser/pages/`.
- **Do this:** `Bun.serve` twice (ports 5173 and 3000) with one shared `fetch`
  handler. Static files come from `pages/`. Each response sets header
  `x-fixture-port: <port>` and every HTML page prints the port and
  `location.origin` in a visible `<h1 id="whoami">`. Routes handled in code:
  - `/redirect/path` → `302` to `/landing.html`
  - `/redirect/origin` → `302` to `http://127.0.0.1:<same port>/landing.html`
  - `/redirect/chain` → `302` to `/redirect/path`
  - `/slow?ms=N` → waits N ms, then serves `landing.html`
  - `/status/404`, `/status/500` → that status with a short body
  - `/set-cookie` → sets `fixture_session=<random>; Path=/; Max-Age=86400`, shows it
  - `/read-cookie` → shows the `fixture_session` cookie or "none"
- **Verify:** `bun tests/fixtures/browser/serve.ts &` then
  `curl -si http://localhost:3000/redirect/origin | head -5` → `HTTP/1.1 302` and
  `Location: http://127.0.0.1:3000/landing.html`.
- **If it fails:** a port in use means a dev server is running; stop it or ask.
  Do not change the port numbers — later tasks cite them.

### Step 2 — Create the pages

- **Goal:** one page per behavior the later tasks test.
- **Why / invariant:** each page tests one thing and shows a **document counter**
  (`sessionStorage`-independent: a random id generated at script start, printed
  in `#doc-id`). A changed `#doc-id` is how checks tell "same document" from
  "reloaded". Keep it on every page.
- **Files:** `tests/fixtures/browser/pages/*.html`.
- **Do this:** create these pages (plain HTML + inline script, no build step):

| File | Contents |
| --- | --- |
| `landing.html` | `#whoami`, `#doc-id`, links to every other page |
| `spa.html` | buttons calling `history.pushState`, `history.replaceState`, and setting `location.hash = "#/route-b"`; shows current route; doc id stays the same |
| `form.html` | text input, textarea, long page (3000px) for scroll checks |
| `frames.html` | two same-origin iframes each containing a button with the **same** selector `button.save`, text "Save A" / "Save B" |
| `cross-origin.html` | iframe pointing at `http://127.0.0.1:<port>/landing.html` |
| `shadow.html` | custom element with an **open** shadow root containing `button.inner` "Inner"; a second element with a **closed** root |
| `nested-scroll.html` | a 400px scroll container with 50 items, each `button.item` |
| `responsive.html` | layout that changes at 600px width |
| `dialog.html` | button opening a native `<dialog>` with `showModal()` |
| `popup.html` | button calling `window.open('/landing.html','p','width=400,height=400')`, a `target=_blank` link, and a timer that calls `window.open` after 2 s without user activation |
| `hostile.html` | script that, on load, tries to call `window.__TAURI_INTERNALS__?.invoke('browser_host_message', {payload:{__er_composer_submit:true, text:'forged'}})`, dispatches synthetic `click` and `keydown` events every second, sets `window.__er_injected = true`, and logs every attempt into `#log` |
| `canvas.html` | a 600×400 canvas with drawn shapes |
| `password.html` | password input, hidden input with `value="secret-token"`, and `data-token="abc"` attribute |

- **Verify:** open `http://localhost:5173/landing.html` in Safari; every link loads
  and shows its port, origin and a doc id.
- **If it fails:** pages are static; fix the HTML. Do not add frameworks.

### Step 3 — Add the proof log

- **Goal:** one place where proof tasks record results.
- **Files:** create `internal-docs/browser-feature-refactor/proof-log.md`.
- **Do this:** headings `## T02 parking`, `## T03 content world`,
  `## T04 editor surface`, `## T05 navigation, capture, data store`. Under each,
  a "Environment" line template (`macOS version, Tauri 2.x.y, wry x.y.z, Mac model`),
  "Result: pending", and "Observations".
- **Verify:** file renders; headings match.

### Step 4 — Document how to run

- **Files:** create `tests/fixtures/browser/README.md` with the run command and
  the page table from Step 2.
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" just docs-check` → `docs-check: ok`.

## Rollback

Delete `tests/fixtures/browser/` and `proof-log.md`.

## When to stop and ask

- Any port other than 5173/3000 seems necessary.
- You are tempted to change product code to make a page work.
