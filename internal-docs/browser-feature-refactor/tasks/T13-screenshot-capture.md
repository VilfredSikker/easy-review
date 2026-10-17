# T13 — Screenshot capture pipeline

Status: ready-for-agent
Depends on: T05 (snapshot proven), T10 (runtime), T11 (drafts), T12 (assets)
Size: 2–3 days
Risk: high (evidence attached to the wrong target or page misleads the agent)

> **For the executor:** Build exactly this, in order. If T05's proof log says the
> snapshot omits content-world drawing or includes the shell, stop and ask.

## In plain terms

- **The problem.** Notes saved from the page carry no picture. The only screenshot
  path asks the reviewer to pick a screen to share and crops it assuming the
  screen matches the app window.
- **The fix.** After the reviewer selects an element, the app asks the page to
  show just the blue outline and the marker number, takes a picture of exactly the
  page, crops around the element with some context, shrinks it, and attaches it to
  the draft — but only if nothing moved or navigated in between. The reviewer sees
  it before saving and can remove or retake it.
- **TL;DR.** Every note gets a correct, cropped picture of what was selected.

## Definition of done

- [ ] On select, the draft shows a capture preview within 2 s; Remove image and
      Recapture work; Save without image works when capture fails.
- [ ] The image shows the outlined target and its marker number, not the editor,
      hover box, cursor or other markers, and never the app shell.
- [ ] At Retina and window zoom 125% the outline in the image lines up with the
      target (± 2 px at 1×).
- [ ] Navigating, scrolling, resizing or zooming during capture discards the
      in-flight image; a retry uses fresh geometry.
- [ ] `getDisplayMedia` is no longer used for annotations.
- [ ] Tests in Step 6 pass.

## Before you start

- **Native snapshot:** `browser_native::snapshot(&tauri::Webview) -> PNG bytes` (T05).
- **Runtime:** T10 runtime; add the capture messages below to it and to the T08
  envelope.
- **Draft:** T11 `BrowserDraft` (`evidence_rev`, `capture`).
- **Assets:** T12 asset validation and limits.
- **Current shell capture to retire:** `desktop-ui/src/lib/components/AnnotationComposer.svelte`
  (uses `getDisplayMedia`; see `grep -n getDisplayMedia` in that file).
- **Model:** ChatGPT's handshake and crop constants, verified in
  `reference/evidence.md`, "Screenshot capture": 1 s ready timeout, crop = target ∪
  marker box, 32 CSS px padding, longest edge ≤ 1280 px.

## Scope

- **In scope:** capture coordinator in Rust (`crates/er-desktop/src/browser_capture.rs`),
  runtime capture mode, envelope variants, crop/resize, draft wiring, preview UI,
  retiring display capture for annotations.
- **Out of scope:** prompt format (T14), er-mcp (T15). Adding any crate other
  than `image` (already in the lockfile) — stop and ask instead.

## Steps

### Step 1 — Runtime capture mode

- **Do this:** host → runtime `PrepareCapture{req, number}`: hide hover box,
  overlay cursor, region-drag box and every marker except the draft's; draw the
  target outline and marker `number`; after two animation frames reply
  `CaptureReady{req, target_rect, marker_rect, viewport, scroll, scale, zoom, geometry_rev, skip}`
  (`skip = true` when the target is unresolved). `ClearCapture{req}` restores
  normal rendering. Any scroll, resize, zoom or mutation affecting the target
  increments `geometry_rev`.
- **Verify:** `cargo test -p er-desktop browser_messages` passes with the new
  variants and their bounds tests; in the app, sending `PrepareCapture` from a
  debug command hides the hover box and other markers until `ClearCapture`.

### Step 2 — Coordinator

- **Do this:** `capture_for_draft(draft_id)`:
  1. Freeze `(document_id, evidence_rev)` from the draft; hide the editor surface.
  2. Send `PrepareCapture`; wait ≤ 1 s for `CaptureReady` matching `req`.
  3. If `skip`, record `ImageEvidence::Unavailable{reason:"target not resolved"}`.
  4. Call `browser_native::snapshot` **outside the App lock**.
  5. Re-check: same document, same `evidence_rev`, and no newer `geometry_rev`
     reported since `CaptureReady`. Otherwise discard and report "page changed".
  6. In a `finally`-style guard: send `ClearCapture`, show the editor again.
  Whole operation settles in ≤ 5 s.
- **Verify:** the Step 6 tests for stale `geometry_rev`, changed document and
  timeout pass; in the app, killing the page's runtime (navigate away mid-capture)
  leaves the draft with "Screenshot unavailable" within 5 s.
- **Why / invariant:** an image is admitted only if it was taken of the same
  document, target and geometry the draft holds. A late image is dropped, never
  attached to a newer draft or document.

### Step 3 — Crop and resize

- **Do this:** scale = image width / viewport CSS width (use actual image size,
  not assumed device scale). Crop = union(target_rect, marker_rect) padded by
  32 CSS px, multiplied by scale, clamped to the image. Resize so the longest edge
  ≤ 1280 px. Store `crop {x, y, w, h, scale}`, viewport, marker rect in
  `ImageEvidence::Captured`. Use the `image` crate: `Cargo.lock` already contains
  `image 0.25.10` (pulled in indirectly by the clipboard stack), so add
  `image = { version = "0.25", default-features = false, features = ["png"] }` to
  `crates/er-desktop/Cargo.toml`; it adds no new crate to the build. Do the crop
  and resize in Rust, outside the App lock.
- **Verify:** `grep -c 'name = "image"' Cargo.lock` still prints `1` after the
  change (one version only), and the crop unit tests in Step 6 pass.

### Step 4 — Draft and preview

- **Do this:** capture runs automatically after selection when the platform
  reports snapshot support (other platforms: "Screenshot unavailable on this
  platform"). The editor shows the preview, Remove image, Recapture. Editing note
  text keeps the image (it bumps `text_rev`, not `evidence_rev`). Changing the
  target bumps `evidence_rev` and drops the image.
- **Verify:** select, wait for the preview, type a note → preview stays; choose
  "Replace target" → preview is replaced by a new capture; Save → the annotation's
  `image` is `Captured` with an `asset_id` and the PNG exists under
  `browser-assets/`.

### Step 5 — Retire display capture

- **Do this:** remove the annotation use of `getDisplayMedia` in
  `AnnotationComposer.svelte`. If the component has no other user afterwards,
  delete it (T16 checks).
- **Verify:** `grep -rn "getDisplayMedia" desktop-ui/src` returns nothing related
  to annotations; `bun run check` and `bun run lint` pass.

### Step 6 — Tests

- **Rust unit tests:** crop math at scale 1, 2, and 2 × 1.25 zoom; clamping at
  image edges; longest-edge resize; stale `geometry_rev` → discarded; different
  `document_id` → discarded; timeout → `Unavailable`, draft kept.
  `cargo test -p er-desktop browser_capture` (through `locked.sh`).
- **Real app:** `form.html`, `nested-scroll.html` (scroll during capture),
  `spa.html` (pushState during capture), Retina display and 125% zoom pixel check.

## Rollback

Revert; drafts save without images.

## When to stop and ask

- The snapshot includes the editor surface or the shell.
- Adding `image` pulls a second version into `Cargo.lock`.
