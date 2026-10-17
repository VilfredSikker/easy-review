# ChatGPT 26.1007 in-app browser: main process (from re-main)

Source: cgpt-pretty/main-CqEoQDOU.js line numbers. [O] observed, [I] inferred.

## Summary (agent's)
1. Pages render in a `<webview>` tag in the app renderer; every tab shares one profile `persist:codex-browser-app`. Main overwrites the guest partition and hardens webPreferences on every attach. Guests bound to their tab by an instance id plus a renderer instance id and host generation encoded in the partition string. Attach-token URL only filters history in main.
2. Popup adoption, `clone()`, moving a page between windows, permission prompts, content insets need APIs that exist only in the bundled "Codex Framework" binary (custom Electron build); not portable to Tauri.
3. window.open: only `foreground-tab`/`background-tab` keep `window.opener`; `new-window` is denied and reopened as a fresh tab, so popup OAuth loses its opener [I]. `canOpenPopup` belongs to the browser-settings webview, unrelated to OAuth.
4. `codex_desktop:browser-sidebar-runtime-message` has no trust check; a zod union of 18 `runtime-*` types is the only limit; handlers find the page by sender webContents id.
5. Screenshots: `capturePage` of the whole guest, crop by viewport size recorded with the annotation, 32px padding, longest side capped at 1280/1024/768. "Browser-use" = agent driving the in-app browser over CDP behind a JSON-RPC socket at `/tmp/codex-browser-use`.

## 0. Corrections and runtime caveat
- Custom runtime: package.json says electron 42.3.0, but app ships `Codex Framework.framework` with `_createWebViewAdoptionLease`, `createWebViewTransfer`, `setAgentActive`, `setPermissionPromptHandler` [O]. Depending on these: popup adoption (78179–78213, 79453–79517), `webContents.clone()` (82871), moving live page to another window (80764), `session.fromPartition(…, {permissionHandling:'browser'})` + prompt handler (65825–65828), `setContentInsets` (83830), `setAgentActive` (78821), `loadURL` `initialRequestHeaders` (79709), `owl-*` events (78701–78705). Stock Electron lacks them [I].
- Attach token (63058) in main only drops matching history entries (63215–63230) [O]; renderer uses it (app-shared 263811).
- `canOpenPopup` (65672) is for browser-settings webview (chrome://settings, extension pages) [O].
- Browser sites: 79453 guest popups, 81806 tab thumbnail, 82010 clipboard screenshot, 67716 and 68475 annotation screenshots. 63775/63863, 64048/64338, 64855/64976 are other webview features. 14587, 122928, 126701 not browser.
- 132374, 134257 deny all popups. 142750 = owner window handler creating comment-popup BrowserWindow. Only WebContentsView (69242–69260) is a hidden download helper [O].

## 1. Guest creation and attach
- Surface: `<webview>` tag in app renderer; main hooks owner's will-attach-webview / did-attach-webview (65590, wired 83615). Owner has webviewTag true only for some windows (142551) [O].
- Forced webPreferences each attach (65607, 65623) [O]: partition `persist:codex-browser-app` + configured session; preload browser-page-preload.js; navigateOnDragDrop true; transparent false; sandbox true; contextIsolation true; nodeIntegration/InSubFrames/InWorker false; webSecurity true; allowRunningInsecureContent false; webviewTag false; plugins false; devTools true; deletes allowpopups/disablewebsecurity/webpreferences params; disablePopups true; backgroundThrottling false for localhost/local file first URL (65455); params.src='' so main owns first navigation (65440).
- Binding [O]: renderer encodes identity in partition `persist:codex-browser-app-route:<enc(conv\0tab)>:host:<rendererInstanceId>:<hostGeneration>` (332, 65110–65146) + data attributes conversation-id / browser-tab-id / page-restore / page-storage-id. Before mount: registerWebviewHostSession + registerWebviewHost RPC (80841–80856); new renderer instance cancels pending attaches and resets hostGeneration; lower generation refused. will-attach checks feature enabled, route/storage/restore match, not tearing down, renderer instance + generation match, storage ownership; stores instanceId → {route, hostGeneration, rendererInstanceId}. did-attach pops entry by viewInstanceId; attachWebview re-checks generation, refuses rebinding webContents to a different route (80910), wires events (78799), closes older webContents for the tab (80948). Same host identity attaching again reattaches existing guest (65381–65408).
- Rejections logged IAB_LIFECYCLE: no route, stale instance/generation, storage/restore mismatch, teardown, adoption lease mismatch, settings webview outside settings route [O].

## 2. Session, profile, network
- One persistent profile for all tabs/conversations/agent tabs (10826 → 65825); languages follow OS [O].
- Permissions: request handler denies, check handler false; real prompts via custom setPermissionPromptHandler → permissionPrompts.show, tied to tab, mountGeneration, navigationVersion (81856+) [O].
- Certificates: no certificate-error handler → Chromium default blocking [I]; ERR_CERT_* shows certificate error page and securityState 'certificate-error' (62881, 79832) [O].
- Request filtering (65839–65855) [O]: deeplink redirect; app-route main-frame requests cancelled and routed in-app; settings allowlist; while agent drives the tab, frames must be http/https/about:blank.
- Hosted Sites `*.chatgpt.site` / `*.appgardenapps.dev`: ID token header on load (78335–78360) [O].
- clearData; downloads store; popup-turned-download closes [O].
- Remote hosts: `localhost:<port>` rewritten to a local port forward; durable hosts refuse (84064–84082) [O].

## 3. Navigation and history
- Commands: renderer → `codex_desktop:message-from-view` `{type:'browser-sidebar-command', conversationId, browserTabId, command}` (86688–86802): navigate, go-back, go-forward, reload{ignoreCache}, stop, reset, close-tab, scroll; find (open/set-query/next/prev/close); zoom (step/set-percent/reset); print, capture-screenshot, set-interaction-mode, annotation commands, transfer-conversation. Mouse back/forward from guest via runtime-mouse-navigation [O].
- Navigate (79634–79745): '' → about:blank; compare after `new URL().href`; no webContents → pendingUrl; restricted URL keeps current; else bump navigationVersion (drops stale async results) and loadURL (replaceCurrentEntry when only entry is about:blank); "navigation already pending" retried every 50ms ×100; ERR_ABORTED/-3 ignored [O].
- Back/forward check canGoBack/canGoForward then call webContents; reload/stop direct; reload on error page reloads failedUrl [O].
- Events (79425–79453): did-start-loading, did-start-navigation, did-stop-loading, did-finish-load, did-navigate, did-navigate-in-page, did-redirect-navigation, did-fail-load, will-frame-navigate, will-navigate, will-redirect, page-title-updated, page-favicon-updated, audio-state-changed, user-media-capture-state-changed, render-process-gone, unresponsive, dom-ready, preload-error, found-in-page, before-input-event, before-mouse-event, context-menu [O].
- State: all events → one function (78898) → snapshot (79746) → `browser-sidebar-state` sent only when changed (deep compare). While loading, URL shown is last committed (79781) [O].
- Redirects update URL; did-navigate-in-page commits main frame only; committing a different page closes the comment editor (83820) [O].
- Load errors: subframe, -3, ERR_ABORTED, ERR_BLOCKED_BY_CLIENT ignored; else localized Chrome-style error page as data: URL loaded with replaceCurrentEntry; error page → failedUrl map keeps history/persistence/address bar on failed URL. Crash/unresponsive page; "Open in external browser" only for https or local http when app isn't default browser [O].
- Schemes: external open only for https or local http; app links in-app for localhost/file and per user prefs; connector OAuth callbacks on app scheme caught as deep links [O].
- Restore via `navigationHistory.restore({entries,index})` (77518) [O].

## 4. Popups
setWindowOpenHandler (79453–79534) in order [O]: 1 app-scheme OAuth callback → deny+queue; 2 app deep link → queue, deny; 3 error page "open externally" → external, deny; 4 browser-use URL policy → deny; 5 disposition `other` → deny; 6 foreground-tab/background-tab → ALLOW outlivesOpener + createWindow, child gets adoption lease, owner gets `open-browser-tab {adoptionLease, adoptedWebContentsId, initiator:'window_open'}`, renderer mounts `<webview>` adopting the child, re-hardened at will-attach; opener kept; 7 other dispositions (new-window, i.e. window.open with features) → deny and reopen URL as fresh tab with NO opener; 8 background-tab without adoption → inactive fresh tab.
- Sized OAuth popups using window.opener lose the opener [I]; redirect OAuth in-tab works. will-navigate/will-frame-navigate/will-redirect apply same checks [O].

## 5. Lifecycle and presentation
- Identity conversationId + browserTabId (`conv\0tab`); browserStorageId persistence key; mountGeneration + hostGeneration guard stale mounts [O].
- `browser-sidebar-sync` (82532–82706): visible, presented, bounds, viewportScale, topContentInset, emulatedViewportSize, themeVariant, mountGeneration, hostKind, isAgentControllingBrowser, runtimeIntlConfig. Applied via Emulation.setDeviceMetricsOverride, setZoomFactor, setContentInsets, Emulation.setEmulatedMedia (prefers-color-scheme follows app theme) [O].
- Hidden: throttling allowed unless visible/captured/agent-driven; visible local pages unthrottled; hiding cancels pending site-annotation permission requests [O].
- Window move/resize refresh cached bounds; blur clears annotation suspension [O].
- Budget: ≤32 live hidden renderers per window; protected if selected in last 30 min, active/presented, loading/restoring, audible/capturing, downloading, agent-driven; others suspended oldest first (81326–81470) [O].
- Suspend/close: persist snapshot → `browser-sidebar-destroy-webview {teardownId, reason}` → renderer removes tag → `browser-sidebar-webview-destroyed {teardownId}` → close webContents. Close deletes persisted page [O].
- Persistence: browser-sidebar-page-states.json, ≤100 pages, 64 MiB, 500 entries/page, zod-validated, atomic temp+rename, flushed on close/session-end (63470–63579) [O].
- Window transfer via createWebViewTransfer (refused while editor/teardown pending); duplicate via clone()+adoption or cold restore [O, custom API].
- Crash page + `browser-sidebar-tab-render-process-gone` [O].
- One shared ref-counted `debugger.attach('1.3')` per guest for annotations, emulation, browser-use; 1s timeout, serialized (62905–63060) [O].

## 6. IPC
- `codex_desktop:message-from-view` renderer→main, gated by isTrustedIpcEvent (sender id registered + frame URL check) (142492) [O].
- `codex_desktop:browser-sidebar-runtime-message` guest→main: NO trust or senderFrame check; only zod union of 18 runtime-* types (67193) [O].
- `codex_desktop:browser-page-event` guest→main: shape {version:1,type}; only site_annotation_features_changed must come from main frame (132037) [O].
- Policy sendSync: annotation policy requires senderFrame === mainFrame (65074); webmcp policy no frame check [O].
- Runtime messages enter the same per-window handleMessage switch as the trusted renderer; schema is the only limit. Handlers resolve the page by sender webContents id (83866); payload ids never trusted; unknown senders no-op; owner-only messages require primary window [O].

## 7. Screenshot capture
- `guest.capturePage()` with no rect (full visible guest, device px), cropped in main (68469–68491) [O].
- Handshake (68493–68646): main → `prepare-comment-screenshot {commentId}`; guest → `comment-screenshot-ready {annotationViewportRect, markerViewportPoint, skipScreenshotCapture}`; waiter matches sender webContents id + commentId; 1s timeout (68796); newer request cancels older; capture or skip; `clear-comment-screenshot` in finally [O].
- Crop: ready-message rect wins; crop = rect ∪ square marker box; scale = image px / annotation.viewportSize (CSS px recorded with annotation); padding 32px (16 compact) before scaling; floor/ceil, clamp; whole image if no rect/viewport (67256, 68847–68857) [O].
- Resize: longest side ≤1280 uncropped, 1024 cropped, 768 compact; PNG data URL + {width,height,cropViewportRect,cropPaddingPx,annotationViewportRect,markerViewportPoint,isCompact} (68862–68902) [O].
- ≥10 comments → older screenshots re-cropped/downsized to 768 (68281) [O].
- Virtual anchors captured before editor opens; abort if page navigates/destroyed (67699–67730) [O].
- Editor overlay = child BrowserWindow opened by renderer via `window.open('about:blank', frameName)`, claimed by main after parent/owner check (142750–142800); positioned at owner content bounds + browser bounds, clamped to work area; while focused, `Emulation.setFocusEmulationEnabled` keeps page looking focused (62974) [O].
- Thumbnail capturePage stayHidden, cached 1s, ≤608×320; clipboard screenshot [O].
- Design provenance via CDP DOM.querySelector + CSS.getMatchedStylesForNode (68643) [O].

## 8. Browser-use and notes
- Agent controls the in-app browser over CDP (Page.navigate, Input.*, Runtime.evaluate, Page.captureScreenshot, Target.*); restricts navigation to http(s)/about:blank; setAgentActive (74103–74155) [O].
- `/tmp/codex-browser-use/*.sock` JSON-RPC 2.0, 4-byte length framing, 8 MiB cap; default authorizer allows everyone (13063) [O].
- WebMCP: ≤100 tools, 64 KiB descriptors [O].
- Copy: forced partition/webPreferences per attach; generation + teardown-id handshakes; navigation version counter; shared ref-counted debugger with timeouts; data: error pages mapped to failed URL; budgeted suspension keeping history; zod-checked guest channel resolved by sender id; crop by recorded viewport size.
- Avoid/note: adoption/transfer/clone need forked Electron; guest channel shares trusted switch; new-window popups lose opener; one cookie jar shared by agent and user tabs.

## Open questions (agent's)
1. Stock Electron 42 vs custom APIs not diffed. 2. What `r.ii` counts as app route. 3. Who connects to browser-use socket. 4. Renderer use of attach-token URL. 5. Permission-prompt UI location. 6. Is `disablePopups` a custom option.

## Answers to preload agent
(a) runtime-message: no frame/document check; page by sender webContents id; a subframe preload would be accepted [I]. browser-page-event: only site_annotation_features_changed must be top frame. Per-document guards: annotation-policy sendSync requires senderFrame===mainFrame and returns disabled if main-frame navigation starts while waiting; permission result dropped if navigation started; pending editor/screenshot closed on did-start-navigation (67699); opening editor re-checks guest getURL() matches anchor pageUrl (67737). No document token [O].
(b) Full capturePage then crop/resize in main; 1s timeout leaves comment without screenshot (non-fatal).
(c) Preload set on every browser webview attach (65607) and adopted popups (79467); top frame only [I]. did-navigate-in-page updates state to renderer; runtime sync re-sent to page only if interactionMode changed (78919); full sync on dom-ready (79179) and every browser-sidebar-sync (83826) [O].
