# T11 — Host editor and host-owned drafts

Status: ready-for-agent
Depends on: T04 (editor surface chosen), T10 (runtime reports targets), T12 (v2 storage)
Merges with: T10, in one PR (build on T10's branch). Deleting the legacy in-page
composer here is safe on every platform because T10 installs the new runtime
everywhere.
Size: 2–3 days
Risk: high (reviewer text must never pass through the page; saves write review data)

> **For the executor:** Do not start unless `proof-log.md` `## T04 editor surface`
> names a chosen surface. Build exactly this, in order.

## In plain terms

- **The problem.** The note editor is drawn inside the web page, so the page can
  read or fake what the reviewer types. A note is shown as "saved" before storage
  confirms it, and an unsaved note is lost when a dialog opens or the tab changes.
- **The fix.** The app owns each draft: which page, which element, the text, and
  any screenshot. The editor is drawn by the app on the surface T04 chose,
  positioned next to the element the runtime reports. Save waits for storage, and
  only then shows the numbered marker.
- **TL;DR.** The reviewer types into the app, not the page, and drafts survive.

## Definition of done

- [ ] Selecting a target (T10 `Select`/`RegionSelect`) creates a host draft and
      opens the editor next to the target's viewport rect, clamped to the pane.
- [ ] The editor moves with the target on scroll/resize (`AnchorUpdate`) and shows
      "Target no longer on page" when status becomes `missing` or `ambiguous`.
- [ ] Escape closes the editor and keeps the draft; Escape again leaves annotate
      mode. Explicit Discard deletes the draft.
- [ ] Drafts survive modals, pane hide/show and tab switches (in-process only).
- [ ] Save writes through T12 storage, is idempotent (retry never duplicates), and
      only a confirmed save closes the editor and shows the marker with its number.
- [ ] A draft keeps its original view bucket; saving after the bucket changed is
      refused with a message asking to return to that view.
- [ ] `LegacyComposerSubmit` (T08), `AnnotationComposer.svelte`'s in-page path and
      `onComposerSubmit` in BrowserView are deleted.
- [ ] `hostile.html` cannot cause a save; typing in the editor never reaches page
      key listeners.

## Before you start

- **Editor surface:** the prototype from T04 (child webview over the browser, or a
  borderless child window) and its capability file.
- **Current flow to replace:**
  - `desktop-ui/src/lib/components/BrowserView.svelte` — `onComposerSubmit`
    (line 499) turns a page message into `submitAnnotation`; `submitAnnotation`
    (line 608) calls `app.cmd("add_ui_annotation", …)` with
    `screenshotDataUrl: null` and calls `showSavedPopover` **before** the command
    resolves; `openPageComposer` (line 299), `closePageComposer` (line 320).
  - `desktop-ui/src/lib/components/AnnotationComposer.svelte` — shell composer with
    display capture (`getDisplayMedia`); its capture is replaced by T13.
  - `crates/er-desktop/src/commands.rs:10210` — `add_ui_annotation` (validates the
    view with `abort_wrong_view`, line 288; loads, pushes, saves under the App lock).
  - View identity: `crates/er-desktop/src/snapshot.rs:165` — `OptimisticView
    { active_tab, repo_root, pr_number, branch, mode }`.
- **Storage:** T12's `save_browser_annotation` (idempotency key, asset IDs).
- **ADRs:** 0018 (commands ingest snapshots), 0034 (derived revision for new
  snapshot fields), 0015 (no slow work under the App lock), 0004 (per-view scoping).

## Scope

- **In scope:** a draft store in Rust (`crates/er-desktop/src/browser_drafts.rs`),
  draft commands, the editor UI on the T04 surface, BrowserView wiring, deleting
  the in-page composer path.
- **Out of scope:** screenshot capture (T13 adds it to the draft), prompt format
  (T14).

## Steps

### Step 1 — Draft store

- **Do this:** `BrowserDraft { draft_id, tab_id, generation, document_id, view: OptimisticView-equivalent bucket key, target (locator + captured geometry from T10), status, text, text_rev: u64, evidence_rev: u64, capture: Option<CaptureRef> }`.
  At most one open draft per tab. Commands: `browser_draft_open(tab_id, target)`,
  `browser_draft_set_text(draft_id, text_rev, text)` (≤ 8 KiB; reject older
  `text_rev`), `browser_draft_discard(draft_id)`, `browser_draft_save(draft_id, idempotency_key)`.
- **Why / invariant:** the draft is bound to the tab, document and view bucket
  that were active when it was opened. Nothing re-targets it silently. A new
  `Select` while a draft is open asks "Replace target?" in the editor; it never
  swaps automatically.
- **Verify:** `cargo test -p er-desktop browser_drafts` (through `locked.sh`) with
  the Step 5 Rust tests written alongside.

### Step 2 — Editor UI

- **Do this:** a Svelte component rendered on the T04 surface: target description,
  note textarea, capture status placeholder (T13 fills it), Save, Discard. The
  backend positions the surface next to the target's viewport rect converted to
  window coordinates (pane origin + rect × zoom), clamped inside the pane, and
  re-positions on `AnchorUpdate`. While the editor has focus the page receives no
  keys (verified in T04).
- **Verify:** on `hostile.html`, select an element and type "abc" in the editor:
  the page's `#log` records no key events; on `nested-scroll.html` the editor
  follows the selected item while the container scrolls.

### Step 3 — Save

- **Do this:** `browser_draft_save` runs outside the App lock for slow work (ADR
  0015): it reads what it needs under the lock, releases, writes through T12, then
  re-takes the lock to revalidate the view bucket and bump the desktop revision.
  On success: delete the draft, emit the saved annotation, show the marker (number
  assigned by storage). On failure: keep the draft and show the error with Retry.
  The idempotency key is generated when the draft is opened.
- **Verify:** make the bucket's `browser-annotations.json` read-only, Save → error
  shown, draft and text kept; restore permissions, Retry → exactly one annotation
  in the file (`grep -c '"id"'`), marker shows its number.

### Step 4 — Remove the in-page composer

- **Do this:** delete `onComposerSubmit`, `openPageComposer`, `closePageComposer`,
  `showSavedPopover`'s pre-save call, the `LegacyComposerSubmit` envelope variant,
  and any remaining composer code in the old frame script.
- **Verify:** `grep -rn "LegacyComposerSubmit\|onComposerSubmit\|openPageComposer\|__er_composer_submit" crates desktop-ui/src`
  returns nothing; `cargo check -p er-desktop` and `bun run check` pass.

### Step 5 — Tests

- **Rust:** draft store — stale `text_rev` rejected; save idempotent (two saves,
  one annotation); bucket change refuses save; discard removes; one draft per tab.
  `cargo test -p er-desktop browser_drafts` (through `locked.sh`).
- **TS:** editor component tests with `bun run test`.
- **Real app:** definition-of-done list on `form.html`, `nested-scroll.html`,
  `hostile.html`, including: type a note, open settings, close it, switch tabs and
  back — the draft and its text are still there.

## Rollback

Revert; the old composer returns only if T08's legacy variant is restored too.

## When to stop and ask

- The editor cannot be positioned without page coordinates the runtime does not
  report.
- Any path would send reviewer text into the page.
