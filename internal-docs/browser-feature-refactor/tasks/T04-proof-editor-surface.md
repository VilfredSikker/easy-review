# T04 — Proof: host editor surface above the page

Status: ready-for-human (needs native focus and input checks)
Depends on: T01
Size: 1 day
Risk: high (decides whether the note text can stay out of the page)

> **For the executor:** Experiment with a yes/no outcome. Record it in
> `internal-docs/browser-feature-refactor/proof-log.md` under `## T04 editor surface`.
> T11 builds the real editor on the surface you choose.

## In plain terms

- **The problem.** The note editor is drawn inside the web page today, so the page
  can read and fake what the reviewer types. We want the app to draw it instead.
  But the page is a native view that covers the app's HTML; an ordinary HTML
  element cannot appear on top of it.
- **The question.** Which native surface can sit above the page, next to the
  selected element, take keyboard focus, and keep its text away from the page?
  ChatGPT uses a separate small popup window for this.
- **TL;DR.** Pick a surface for the editor that floats over the page.

## Definition of done

- [ ] `proof-log.md` `## T04 editor surface` names the chosen surface, the
      rejected one, and the scenario results.
- [ ] Prototype code for the chosen surface exists behind a debug-only command
      and can be shown at a given rectangle and hidden.

## Before you start

- Candidates (try A first):
  - **A. Second child webview** added to the main window **after** the browser
    child, so it stacks above it. Loads an app page (same origin as the shell,
    e.g. `WebviewUrl::App("editor.html".into())`), transparent background,
    sized to the editor card.
  - **B. Borderless child window** (`tauri::WebviewWindowBuilder` with
    `decorations(false)`, `transparent(true)`, `parent(&main_window)`,
    `skip_taskbar(true)`), positioned in screen coordinates.
- Child webviews are created today with `window.add_child(builder, position, size)`
  at `crates/er-desktop/src/browser_webview.rs:186-192`; copy that call shape.
- Capabilities: an app-origin webview needs a capability entry to call commands.
  See `crates/er-desktop/capabilities/` for the existing files; give the probe
  surface its own minimal capability.
- `tauri.conf.json` and `macos-private-api` are already enabled
  (`crates/er-desktop/Cargo.toml`, `tauri` features), which transparency needs.

## Scope

- **In scope:** a probe page (static HTML with a textarea) under the desktop UI's
  public assets or a dedicated entry, a debug-only command to show it at
  `(x, y, w, h)`, a capability file for it, the proof log.
- **Out of scope:** real editor UI, drafts, anything in `BrowserView.svelte`.

## Steps

### Step 1 — Show candidate A over a browser child

- **Do this:** debug command `browser_probe_editor(app, x, y, w, h, show: bool)`
  creating (once) and positioning a child webview labelled `browser-editor-probe`
  over the browser child. Page content: a textarea and a Save button that sends
  the text to Rust with `invoke`.
- **Why / invariant:** the surface must stack above the browser child on every
  show, including after the browser child is recreated or re-shown. If re-showing
  the browser child puts it on top, record it.
- **Verify:** with `form.html` open, the probe appears above the page at the given
  rectangle.

### Step 2 — Run the scenarios (for A, then B if A fails)

1. Click into the textarea and type. Keys go to the textarea; the page's
   `form.html` input does not change.
2. In `hostile.html`, the page's key listeners log nothing while typing in the
   editor.
3. Move the editor 60 times per second for 2 s (devtools loop calling the command)
   — no flicker that hides it under the page, no crash.
4. Press Escape in the editor: the editor's own handler receives it.
5. Hide the editor: focus returns to the page; typing reaches the page.
6. Resize the window and move it between displays with different scale factors:
   the editor stays aligned with the requested logical rectangle.
7. Transparent corners show the page below, not a white box.

### Step 3 — Decide

- **Pick A** if 1–7 pass. **Pick B** if A fails 1, 2 or 3 but B passes. **No** if
  neither passes 1 and 2; then the spec's fallback applies (page sends geometry
  only, the editor is shown beside the browser pane rather than over it) and the
  owner must approve that UX.

## Rollback

Remove the probe command, page and capability.

## When to stop and ask

- Neither surface keeps keystrokes away from the page.
- Stacking order changes unpredictably when the browser child is shown.
