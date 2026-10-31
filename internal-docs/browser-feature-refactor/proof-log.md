# Browser native proof log

All results remain pending until observed in a real macOS Tauri window.

## T02 parking

Result: **pending**. No mechanism is selected and no dependent gate is released.
Scaffold prepared on Linux on 2026-10-31; native compilation and every observation
below still require an owner Mac. Fill in macOS version, hardware, display scale,
Tauri/Wry versions, date, and repeated observations before changing this result.
The current lockfile pins Tauri 2.12.1, Wry 0.57.0, objc2 0.6.4 and
objc2-app-kit/objc2-web-kit 0.3.2.

The probe retains the existing WKWebView and reports its address as
`nativeIdentity`; matching addresses alone do not prove document preservation.
HideOnly changes visibility; HideAndResign also resigns focus and moves to a
nonzero 1×1 frame; Detach additionally retains and removes the child from its
superview. Unpark restores the recorded native frame, sibling position when
reattaching, visibility, then saved first responder. Unlike a future product
adapter, it restores recorded geometry rather than taking new shell bounds.
Only one child can be parked at a time; duplicate park and variant changes fail.
Close the probe after any error that prevents restoration. Retained native state
is discarded on explicit fixture close or main-window destruction. Existing tab
children can also be targeted; stale identity or a missing child discards their
metadata when next probed.

The product still destroys its registered browser children before real modals.
To avoid confusing that destruction with a parking failure, `fixtureUrl` creates
one dedicated `parking-proof` child in the same Tauri window and native child
hierarchy, outside `BrowserWebviewState.created`. Repeated calls with fixtureUrl
select this existing child without navigating it. The fixture can therefore stay
parked while a real modal exercises the unchanged overlay path. This is a debug
harness, with no product browser hooks. Omit fixtureUrl only for a supplementary
existing-tab check; that tab cannot survive the current real-modal teardown.

Run from the repository root on macOS:

```bash
LOCK=/home/agent/.codex/skills/confidence-merge/scripts/locked.sh
REPO=$(git rev-parse --show-toplevel)
"$LOCK" "$REPO" env CARGO_TARGET_DIR=target/desktop cargo check --locked -p er-desktop
"$LOCK" "$REPO" env CARGO_TARGET_DIR=target/desktop cargo test --locked -p er-desktop browser
"$LOCK" "$REPO" env CARGO_TARGET_DIR=target/desktop cargo check --locked --release -p er-desktop
"$LOCK" "$REPO" just clippy
bun tests/fixtures/browser/serve.ts
# In another terminal:
./scripts/tauri-dev.sh
```

Use the installed shared lock path when available; otherwise use the owner's
normal serialized check wrapper. In the **main shell's** devtools console:

```javascript
const invoke = (args) => __TAURI_INTERNALS__.invoke('browser_probe_park', args);
const fixtureUrl = 'http://localhost:5173/landing.html';
const probe = (variant, park) => invoke({tabIdx: 0, variant, park, fixtureUrl});
await probe('HideOnly', false); // creates the dedicated child once
```

In the child, click the Form link to establish Back history, type distinct text
into both fields, scroll halfway, and record visible `#doc-id`. Focus its text
field immediately before parking. Keep the window geometry fixed during a run.
Run each variant independently; compare returned nativeIdentity throughout:

```javascript
await probe('HideOnly', true);
// Perform real clicks, typing, window switching or modal checks while parked.
await probe('HideOnly', false);
// Repeat with 'HideAndResign' and 'Detach'.
for (let i = 0; i < 50; i++) {
  await probe('Detach', true);
  await probe('Detach', false);
}
// Close before changing fixtureUrl; this also releases retained fixture metadata.
await invoke({tabIdx: 0, variant: 'Detach', park: false, close: true});
```

For the native-dialog row, close the child, change fixtureUrl to
`http://localhost:5173/dialog.html`, create it again, and click Open dialog before
parking. Recreate via landing.html and prepare form/history again for other rows.
For modal rows, open settings (and any available nested modal) through the shell,
exercise pointer input, typing and Escape, close all modals, then unpark. Verify
that the fixture's text, document ID, scroll and Back list survived. Inspect the
child devtools or visible fixture values for page input changes; promise success
is never evidence that mouse or keyboard input was blocked.

| Real macOS scenario | HideOnly | HideAndResign | Detach |
| --- | --- | --- | --- |
| Parked child gets no clicks/keys; shell underneath receives input | pending | pending | pending |
| Real modal and nested modal receive clicks, typing and Escape | pending | pending | pending |
| Unpark preserves document ID, form text, scroll and Back history | pending | pending | pending |
| 50 rapid cycles: same native identity, no crash/focus theft, interactive | pending | pending | pending |
| Page's open native dialog survives park/unpark and stays interactive | pending | pending | pending |
| Another app/window and back while parked: no focus/input leak | pending | pending | pending |

Record per-row observations and repetition counts, including frame/hierarchy
changes made externally by Wry. A re-shown or reattached child, refusal to restore
focus, timer/retry requirement, or intermittent failure cannot be a yes. Select
a variant only after repeated success in every row; preserve unknown evidence.

Linux verification: desktop `browser` tests stop in glib-sys because
`glib-2.0.pc` is unavailable, before compiling this crate. GTK/WebKit and a macOS
SDK/target are unavailable. An isolated temporary harness compiling the unchanged
Variant/parser, fixture URL validator and their source tests against url 2.5.8
passed 2 tests; it excludes Tauri/native code and proves no native behavior.
Locked upstream source inspection confirmed Tauri's main-thread callback and
WKWebView pointer contract, the generated objc2 methods/features used here, and
Wry's retained child/frame/visibility implementations. This is API source
validation, **not** a macOS compilation result. The module/import/handler and
cleanup hook are cfg-gated out of release; the permission name has no release
handler. A release Mac check and attempted invocation must still confirm the
command is unavailable. No product overlay, browser bounds or teardown code changed.

## T03 content world

Environment: macOS version, Tauri 2.x.y, wry x.y.z, Mac model

Result: pending

Observations: pending

## T04 editor surface

Environment: macOS version, Tauri 2.x.y, wry x.y.z, Mac model

Result: pending

Observations: pending

## T05 navigation, capture, data store

Environment: macOS version, Tauri 2.x.y, wry x.y.z, Mac model

Result: pending

Observations: pending
