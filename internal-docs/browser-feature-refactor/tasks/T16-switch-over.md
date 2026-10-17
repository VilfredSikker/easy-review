# T16 — Switch over and delete the old path

Status: ready-for-agent
Depends on: T01–T15 done and their real-window checks recorded
Size: 1–2 days
Risk: high (deletes code and changes what every user sees)

> **For the executor:** Do not start until every task's `Status:` is `done`. Build
> exactly this, in order.

## In plain terms

- **The problem.** After the rebuild, the old pieces are still in the code: the
  injected page script, the in-page note editor, the automatic proxy, the v1
  annotation commands, and the permission that let web pages call the app.
- **The fix.** Run the full acceptance list once, then delete everything the new
  path replaced, so there is one browser implementation.
- **TL;DR.** Prove it works end to end, then remove the old browser.

## Definition of done

- [ ] Every row of the acceptance table in `reference/spec.md` ("Acceptance
      checks") is recorded as passed in `internal-docs/browser-feature-refactor/proof-log.md`
      under a new `## T16 acceptance` heading, with date and environment, on macOS.
- [ ] Deleted (or confirmed already gone): `FRAME_SCRIPT` use on macOS and its
      composer code; `inject_frame_script`; `FRAME_SCRIPT` itself; any
      `browser-remote.json` entry not limited to `"platforms": ["windows", "linux"]`;
      the automatic proxy fallback in `BrowserView.svelte`; v1 annotation
      commands `add_ui_annotation`, `delete_ui_annotation`,
      `clear_ui_annotations`, `clear_ui_annotations_for_page`,
      `update_ui_annotation_anchors`, `save_annotation_screenshot`,
      `read_annotation_screenshot`, `list_ui_annotations` and their registrations;
      `load_ui_annotations_cached`; `UiAnnotation`/`ErUiAnnotations` if no reader
      remains; `AnnotationComposer.svelte` if unused.
- [ ] Windows and Linux build; their UI states which capabilities are missing
      where the action appears (annotate, capture).
- [ ] `just lint` passes (fmt-check, clippy, check-ui, lint-ui, docs-check,
      comments-check), and `just test`, `just test-desktop`, `just test-ui`,
      `cargo test -p er-mcp` pass — run one at a time through `locked.sh`.
- [ ] Release notes entry drafted in `RELEASE_NOTES.md` under the release branch's
      section.

## Before you start

- Resolve the release branch (see README "Shared orientation"); this task's PR
  targets it.
- Command registrations: `crates/er-desktop/src/main.rs` `invoke_handler` list
  (annotation commands around lines 2053–2062, browser commands around 2069–2077
  at the time of writing — re-grep).
- Proxy scheme registration: `crates/er-desktop/src/main.rs` around line 1777
  (`register_uri_scheme_protocol("erp", …)`). Keep it only if T07 kept an explicit
  developer proxy; otherwise delete it and `crates/er-desktop/src/browser_proxy.rs`.
- Frontend users of v1 types: `grep -rln "UiAnnotation" desktop-ui/src` (at the
  time of writing: `types.ts`, `optimisticLocal.ts`, `RightPanel.svelte`,
  `AnnotationOverlay.svelte`, `UiAnnotationsCard.svelte`, stories and tests).

## Scope

- **In scope:** deletion of replaced code, platform capability messages, final
  acceptance run, release notes.
- **Out of scope:** new features; any behavior change not listed.

## Steps

### Step 1 — Acceptance run

- **Do this:** walk the spec's acceptance table with the fixture server and the
  real app. Record each row. Any failure: stop; fix it in the owning task's area
  as a separate commit and re-run that row.

### Step 2 — Delete

- **Do this:** delete in this order, building after each: frontend v1 annotation
  UI and types → v1 commands and registrations → `load_ui_annotations_cached` →
  engine v1 functions if unused → old frame script paths → proxy fallback →
  capability file. Use `grep -rn` for each symbol to confirm no reader remains.
- **Why / invariant:** no user-visible feature may disappear; each removed piece
  must have a v2 replacement already in use. If a symbol still has a reader that is
  not replaced, stop and ask.
- **Watch out for:** the TUI (`crates/er-tui`) may read v1 annotations through the
  engine; `grep -rn "ui_annotations\|UiAnnotation" crates/er-tui` before deleting
  engine types.

### Step 3 — Other platforms

- **Do this:** where annotate or capture is unsupported, the button shows a short
  reason ("Not available on Windows yet") instead of failing silently.

### Step 4 — Full gates

- Run each command in the definition of done, one at a time through
  `~/.claude/scripts/locked.sh`. Prune lint baselines only if a fixed violation
  makes them stale (`just clippy-prune`, `just lint-ui-prune`); never add to them.

### Step 5 — Release notes

- **Do this:** a short entry: the browser keeps pages alive across dialogs, Back /
  Forward / Stop, annotations with screenshots, er-mcp tool, popups open in the
  default browser, old `ui-annotations.json` no longer read.

## Rollback

Revert this task's commits; the old path returns alongside the new one.

## When to stop and ask

- Any acceptance row fails.
- A deleted symbol turns out to have a reader with no replacement.
