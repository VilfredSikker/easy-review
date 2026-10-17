# ChatGPT 26.1007 browser page preload (from re-preload)

Line refs: cgpt-pretty/browser-page-preload.js; main:N = cgpt-pretty/main-CqEoQDOU.js. (O) observed, (I) inferred.

## Summary (agent's)
1. Runtime is in the isolated world; pages cannot reach the IPC channels; no per-document token. An optional `document.oai.annotation` main-world API exists when a host policy flag allows it. Page↔runtime is JSON CustomEvents; every input handler checks `isTrusted`.
2. Overlay: open shadow root on a fixed host on `<html>`, top layer via `popover=manual`, re-attached if removed. Picking uses a full-screen blocker and hides the page cursor. Hover box 2px `#0069FB`; markers are numbered `#339cff` teardrops.
3. Picking walks open shadow roots and same-origin iframes and chooses the nearest interactive/text-bearing element. Region drag, text selection, Shift multi-select. Canvas works only if the page registers a surface.
4. Anchor: CSS selector, frame path, name and nearby text, document-coordinate rect, scroll containers. Re-anchoring scores selector matches. Elements not found on the same page are dropped silently.
5. Extraction: tag, role, aria-label, text ≤80 chars, ~25 computed styles; no React component names. Password/masked text excluded from selections. Design edits = injected `!important` stylesheet keyed by a data attribute. The comment editor is drawn by the host. For a screenshot the page draws only the target highlight and reports rect + marker point.

## 1. Isolation and transport
- Preload runs in the isolated world; uses `ipcRenderer` and `contextBridge` (26289, 6499) (O). Guest prefs: sandbox, contextIsolation true, nodeIntegrationInSubFrames false (main:65622–65634) (O).
- Never calls `exposeInMainWorld`. Uses `contextBridge.executeInMainWorld({func,args})` at 6499 (surface registry), 6514 (site API), 7067 (`attachShadow` patch) (O).
- Main world sees, only when host policy enabled + secure context + top frame (26289, 26299, 6187): `document.oai.annotation` (6624–6634) with `toggle(force?)` (user activation), `isActive()`, `request(elementOrRange, {...})` (user activation), `registerControls(...)`, `registerSurface({element, hitTest, renderSelection})` (6227).
- `attachShadow` wrapped: each new open shadow root dispatches `codex:annotation-shadow-root-attached` (7111–7127). Disposers restore props only if unchanged (6639–6648) (O).
- Document CustomEvent `oaiannotationmodechange {active}` (26505); `oaiannotationcontrolchange` on targets `{callback,value,action: preview|preview-original|reset}` (21009–21016) (O).
- IPC channels (6088–6091) (O): `codex_desktop:message-for-view` (host→runtime); `codex_desktop:browser-sidebar-runtime-message` (invoke, runtime→host, all `browser-sidebar-runtime-*`) (26392); `codex_desktop:browser-page-event` (send; telemetry, `webmcp_changed`); `codex_desktop:get-browser-annotation-api-policy` (sendSync at boot → enabled, siteAnnotationPermissionGranted, siteAnnotationFeaturesEnabled, isAgentControllingBrowser, isAnnotationModeActive) (26290–26298); `codex_desktop:get-browser-webmcp-policy` (main:65499–65510).
- Runtime→host types: open-editor, open-design-editor, update-anchor, cancel-editor, exit-comment-mode, focus-editor, open/close-comment-preview, comment-screenshot-ready, annotation-suspension, annotation-selection-modifier-state, design-modifier-state, toggle-annotation-mode, request-annotation-permission, image-drag-started/ended, mouse-navigation (O).
- Host→runtime types: sync, prepare/clear-comment-screenshot, capture-text-selection, select-comment, close-editor, design-scrub-changed, create-comment-at-point, create-comment-from-selection, open-design-editor-at-point, annotation-selection-hover-state, annotation-selection-modifier-state, remove-annotation-selection, restore-editor, annotation-permission-result, browser-site-annotation-feature-preview (26397–26465) (O).
- No per-document token on IPC (O); host trusts sender frame (I). Permission requests carry randomUUID requestId (26470) (O).
- Forgery: IPC unreachable from page (O). `codex:open-annotation` dispatchable by page but re-checks user activation, policy, agent control, secure context, host permission; zod limits initialComment 240 chars (O). Surface protocol: JSON strings ≤8 KB, prototype keys rejected (6264, 6421–6434) (O). All pointer/key/click/drag/wheel handlers check `isTrusted` (O). Page content (aria-label, innerText, `oai-annotatable`, `oai-annotation-metadata`) flows into the anchor → prompt-injection surface (I). Overlay shadow root is open on a fixed id (24425, 4744): page can read/mutate it (O).

## 2. Lifecycle
- Boot order (26671): WebMCP watcher; setVisualZoomLevelLimits(1,3); IPC listener → policy → site API; mouse back/forward; image-drag (O).
- Top frame only (nodeIntegrationInSubFrames false) (I). Mount on DOMContentLoaded; idempotent flags (26359–26361) (O). React bundled, no network fetch (O).
- Pre-subscribe buffering: latest `sync`, one pending editor command, capture-text-selection flag; replayed on subscribe (O).
- No explicit ready message; host pushes state, runtime applies latest (O).
- Host element reuses `#codex-browser-sidebar-comments-root`; open shadow root reused + `replaceChildren` (24425) (O).
- Cleanup on trusted non-persisted `pagehide`: dispose site API, remove IPC listener, unmount React, remove host and cursor styles. bfcache keeps alive; `pageshow` re-arms (O).
- SPA navigation not tracked: no pushState patching, no popstate/hashchange, no Navigation API (O). Comments matched at render by origin+path+search, hash ignored (5737–5747) (O). Re-render on host `sync`, DOM mutations, scroll/resize (O). Host re-sending sync on in-page nav (I).

## 3. Overlay UI
- Host `div` on `document.documentElement`, fixed inset 0, pointer-events none, z-index 2147483647 (26092–26105) (O). MutationObserver re-appends if removed/moved and re-raises popover (4225–4244) (O).
- Open shadow root (24425); single `<style>`, no reset sheet (23347–23382) (O).
- Comment-mode interaction layer is `div popover="manual"` + showPopover() (26079–26087) → above page dialogs; re-raised on `toggle`/`fullscreenchange` (24145) (O).
- Pointer-events: host none; auto only in comment mode without open editor (25706–25719). `.interaction-blocker` covers viewport. Hit test: set overlay to pointer-events none, elementFromPoint, restore in finally (4666–4689) (O).
- Page cursor hidden via injected `<style data-browser-comment-cursor-style>` `cursor:none; user-select:none` into each same-origin document; custom SVG cursor drawn (bubble / + / I-beam) (4186–4212, 23384–23425) (O).
- Hover box: `2px solid #0069FB`, bg #0069FB 3%, inset 1px white .28, 150ms opacity, element's border-radius (23348) (O). Region boxes dashed 2px; text-selection rects `#128dff66`; flex-gap overlay; Google Docs special case (O).
- Markers: `<button data-browser-comment-marker>` teardrop `border-radius:50% 50% 50% 3px`, `#339cff`, white 1px ring, 26px; bold 11px white number = index+1; aria-label "Browser comment {n}"; overlapping markers fanned out; draft marker; hover → host preview; click → edit editor; reduced-motion respected (O).
- Comment editor is NOT in the page: runtime sends `open-editor`/`open-design-editor` with anchorState {viewportRect, viewportPoint, viewportSize, cardViewportRect, themeVariant, visualViewport}; host renders and repositions via `update-anchor` (24433–24438, 25592) (O).
- Element metadata tooltip (tag/size/color/font) only on localhost/127.0.0.1/::1/*.localhost (21100–21119) (O).
- Keyboard: Escape cascade — clear surface selection / cancel text drag → cancel region drag → cancel create editor (`cancel-editor`) → `exit-comment-mode` (25396–25413). Hold Space suspends annotation (`annotation-suspension`). Alt = design picking. Shift = multi-select add (O).
- Non-interference: capture-phase listeners on each same-origin document stop mouse/pointer/click/contextmenu/wheel in comment mode; wheel forwarded by manually scrolling ancestor under cursor; hover suppressed during scroll (+100ms); overlay excluded from picks (O). Page window-capture listeners could run first (I).
- Browse mode: back/forward mouse buttons → `mouse-navigation`; image dragstart → `image-drag-started` (O).

## 4. Target picking
- Hit test (4108–4127): elementFromPoint, descend open shadow roots via shadowRoot.elementFromPoint; recurse into same-origin iframes with offset; cross-origin stops at iframe (O). Listeners attached to every same-origin frame incl. iframes in shadow roots; re-attach on iframe load (4145–4185) (O).
- Useful element (4594–4618): walk up to body; first nonzero box = fallback; return first a/button/input/textarea/select/label/img, or [role], or has text and ≥24×16; else fallback to outermost ancestor ≥48×24; html/body only when hit directly (O).
- Site overrides: `[oai-annotatable]` in `[oai-annotation-container]`; `[oai-annotation-container-text]` → native text selection (O).
- Region: drag ≥4px → region anchor, editor opens with wantsScreenshot (O).
- Text selection: getComposedRanges for shadow DOM; input/textarea selections; >20,000 chars rejected; painted with CSS Custom Highlight API (O).
- Canvas: none built-in; pages opt in via registerSurface (hit-test answered within 250ms → virtual anchor) (O).
- Shift multi-select; right-click caches pick for 5s for host create-comment-at-point (O).

## 5. Anchors
- Kinds: element, text, region, virtual (3880–3932) (O).
- Common fields: pageUrl, frameUrl, framePath, title, elementPath, point {xPercent,y}, rect, isFixed, role, name, selector, nearbyText, scrollContainers (O).
- Element: runtimeElementId (UUID via WeakMap, per document); selector = tag + #id (stop if unique) + ≤2 classes + :nth-of-type, shadow hosts chained `shadow:a>>>b`, requireUnique false, ≤4 levels (4329–4370); framePath = iframe selectors top-down; elementPath ≤4 tags; title = aria-label | 80 chars text | tag; nearbyText = own text or nearest section/article/main/form/nav/aside/header/footer; rect/point in document coords unless fixed/sticky; scrollContainers selector+offsets; site extras elementName/elementMetadata (O).
- Text: textLocator {kind dom|form-control, unique selector, shadowHosts[], start/endOffset, rangeText, direction} + selectionRects, selectedText (O).
- Region: rect + scroll containers. Virtual: surfaceId, targetId, name, metadata, viewport rect (O).
- Re-anchor element (5653–5764): cached live element if connected+visible; else querySelectorAll in framePath frame, keep visible; narrow by exact name then nearbyText; tie-break score name+1000, nearbyText+500, title+250, elementPath+25, minus Manhattan distance from stored rect; write fresh rect back (O).
- Text re-anchor requires exact rangeText and frame URL match (O).
- rAF-coalesced scroll/resize listeners on window, visualViewport, frames, scroll containers; MutationObserver filtered to layout attributes (O).
- Highlights clipped through overflow ancestors and iframes; transform/contain containing-block rules (O).
- Missing: same-page with selector → marker dropped; other page → stored rect; open editor → placement top-left; screenshot → skipScreenshotCapture (O). No "ambiguous" status; scoring picks one silently (O).

## 6. Element description
- Anchor + designEditorState: tag, targetLabel, text {value, previousValue} when direct-text only, computed declarations (color, background-color, font-size/family/weight, border-radius/color/width, padding-*, margin-*, width, height, opacity; flex props for flex containers), site annotationControls (O).
- Also themeVariant, viewport, visualViewport, `<meta name="oai-annotation-editor-default-mode">` (O).
- Not collected: outerHTML, attribute dump, React fibers/components, source locations (O).
- Limits: names 80 chars; selections 20,000; Google Docs text 2,000; site metadata ≤6 keys/64-char keys/256-char values/2,048 bytes; initial prompt 240; controls ≤12 per registration, 16KB; 64 registrations, 1,024 targets (O).
- Redaction: selections touching -webkit-text-security rejected; form-control selection only email/search/tel/text/url + textarea; names use innerText so input values don't leak; no other redaction (O).

## 7. Design changes
- Alt-click / design mode → draft UUID attribute → host does editing; runtime renders `activeDesignChange` + each comment's `designChange` from sync (O).
- Applied as `<style id="codex-browser-design-draft-style">` `[data-codex-browser-design-group~="id"]{prop:value !important}` only for changed values; text edits replace textContent with original stored in data attribute; "original view" toggle (O).
- Site controls color|select|range|toggle previewed via `oaiannotationcontrolchange` (O).

## 8. Capture preparation
- Host captures; page reports geometry. prepare-comment-screenshot {commentId} → screenshot render mode (hover box, cursor, draft marker, tooltips off; target highlight + extra anchors on; numbered markers kept) → one rAF later `comment-screenshot-ready` {annotationViewportRect (union), markerViewportPoint (÷zoom), skipScreenshotCapture} → clear-comment-screenshot resets (25036–25932) (O).
- Coordinates are top-level viewport CSS px; overlay counter-scaled by zoom (O).
- New text/region/virtual comments set wantsScreenshot true (O).

## 9. Other
- WebMCP `document.modelContext` toolchange → webmcp_changed (O).
- Telemetry of `oai-*` site features (≤8) (O). Feature preview backdrop for site annotatables (O).
- Cross-origin iframes get pointer-events none in text-select mode (O).
- No console/network capture, no devtools hooks (O). rAF coalescing, filtered MutationObservers, skip when hidden (O).

## Open questions (agent's)
1. Does main check senderFrame (top frame, current document) on runtime messages? (re-main)
2. Does host re-send sync on did-navigate-in-page?
3. Which surface renders the editor/preview card? cardViewportRect suggests overlay above the guest (re-renderer)
4. How is comment-screenshot-ready turned into an image? (re-main)
5. How are comments persisted/replayed across reloads?
6. Can page-dispatched register-annotation-controls carry Elements across worlds? (runtime test)
7. Is the preload attached only to top-level guests at main:83708?
