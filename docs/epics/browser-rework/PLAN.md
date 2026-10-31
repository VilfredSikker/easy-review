# Browser rework plan

Scope: ER-21 through ER-36. Target: `release/v0.6.1`, branched from
`release/v0.6.0` at `e668531f` on 2026-10-31. The owner
[release-target addendum](./ADDENDUM-release-0.6.1.md) overrides earlier target
references. Later owner decisions recorded in an ADDENDUM override this plan. Native proof results are pending; implementation readiness is separate
from a passed proof. No browser product replacement is authorized by a scaffold.

## Sources and verified starting point

The owner's Feature Board export identifies the 16 task specifications from
[PR 273](https://github.com/VilfredSikker/easy-review/pull/273), commit `98f1191`,
under `internal-docs/browser-feature-refactor/`. That branch is unmerged. Read the
assigned original task and the reference spec from git; do not merge its old base:

```bash
git show 98f1191:internal-docs/browser-feature-refactor/README.md
git show 98f1191:internal-docs/browser-feature-refactor/reference/spec.md
git show 98f1191:internal-docs/browser-feature-refactor/tasks/T12-storage-v2.md
```

ER21 maps to T01, through ER36 to T16. These documents preserve the detailed
scenario lists. This plan overrides their branch instructions, verification
wrapper location, splits and sequencing. Resolve stale line references against
the current worktree rather than copying old code.

The original audit at `db43ec6c` verified: native children are indexed by tab position;
overlay suspension destroys them; navigation comparison drops ports and fragments;
the annotation runtime uses the page world; remote browser capabilities permit
the legacy message command; BrowserView automatically falls back to the proxy.
The v1 loader maps corrupt JSON to an empty collection; screenshot commands still
accept paths; the v2 module and browser MCP tool do not exist. The existing MCP
feedback resolver supports both PR and local branch buckets. ADR numbers through
0048 are occupied. PR 324 also touches snapshot metadata: do not overlap it with
ER32b without first reconciling the merged state.

Keep the established owner choices: Tauri/native child webviews, macOS first,
buildable and honest Windows/Linux capabilities, no v1 migration or downgrade,
new `browser-annotations.json`, old files left on disk, externally opened
user-activated popups, denied inactive popups, one prompt renderer plus MCP,
no portable bundles, and explicit reviewer action for agent handoff. Saving never
messages an agent. Fix browser bugs within the phase that replaces their code.

## Ready slice and gates

Start ER21 and ER32a in separate worktrees after these planning documents are
committed. They require no native mechanism selection and preserve current
product behavior. After storage review, ER34a and then ER35 can deliver a renderer
and an MCP reader using synthetic v2 fixtures. None proves the browser rebuild.
ER32b may follow once the snapshot lane is free. Preserve working v1 exports until
ER34b integrates the replacement; do not switch export readers early.

ER22–25 may implement debug-only probes on Linux, but remain pending until real
mouse/keyboard observations in the macOS Tauri window are recorded. Scaffold
review/merge does not release the dependent gate. ER26 waits for ER25's selected
native adapter; ER27–29 follow ER26. ER28 also waits for ER27 because its document
validation consumes navigation identity. ER30 and ER31 ship as one combined PR.
If that combined patch exceeds about 1,500 lines, use reviewable commits and ask
the orchestrator to stage preparatory work; never merge the runtime alone.

The earlier review draft contains proposals, not owner decisions. Before affected
integration, record concrete resolutions for: query redaction/matching policy;
how preview markers receive a number assigned only on first successful save;
atomic draft/bucket validation before commit; generation captured by callbacks;
document activation and same-document capture invalidation; reliable load-failure
attribution; popup activation attribution; and delivery-dependent image wording.
Do not silently select the proposed image compositing or bucket coordinator.
Storage/renderer APIs can be tested without production callers while these are
open. Capture and export wiring cannot claim acceptance with contradictory
contracts. The reference spec already requires accurate image-omission notices.

## Verification commands

Run from the assigned worktree. Every heavy check uses this same machine lock;
never start simultaneous suites, even across worktrees. The brief gives setup.

```bash
LOCK=/home/agent/.codex/skills/confidence-merge/scripts/locked.sh
REPO=$(git rev-parse --show-toplevel)
"$LOCK" "$REPO" just docs-check
"$LOCK" "$REPO" ./scripts/er-tui.sh test -p er-engine browser_annotations
"$LOCK" "$REPO" ./scripts/er-tui.sh test -p er-engine browser_prompt
"$LOCK" "$REPO" ./scripts/er-tui.sh test -p er-engine export
"$LOCK" "$REPO" cargo test -p er-desktop browser
"$LOCK" "$REPO" cargo test -p er-desktop snapshot
"$LOCK" "$REPO" cargo test -p er-desktop tabs::
"$LOCK" "$REPO" cargo test -p er-mcp
"$LOCK" "$REPO" just check-ui
"$LOCK" "$REPO" just test-ui
"$LOCK" "$REPO" just lint
```

The entries below name subsets of these exact commands; use the named engine
filter, desktop filter, or MCP suite, then relevant lint gates. For native checks:
start `bun tests/fixtures/browser/serve.ts`, then `./scripts/tauri-dev.sh` on a Mac.
Record environment, mechanism, observations and individual yes/no/partial results
in the proof log. Linux checks and simulated DOM events never satisfy that gate.

## PRs and lanes

IDs are PR tracking IDs, not existing GitHub PR numbers; STATUS records actual
numbers after opening. All branches below start with `epic/` and target the
release branch. Terra means `gpt-6.1-terra`, Sol means `gpt-6.1-sol`, per the owner.
Use Sol reviewers for native, storage, sender validation, prompt and MCP work;
Terra can review fixtures and mechanical snapshot/export wiring. Native lane is
serial because probes share adapter/registration files; storage then prompt are
serial at their shared engine module. Snapshot/export integration is serial with
other PRs touching those hot files. No database lane applies.

| PR ID / branch suffix | Lane/model | Depends on | Scope and acceptance | Out of scope; risk/size |
| --- | --- | --- | --- | --- |
| ER21 / er21-fixtures | Fixtures/Terra | — | T01 dual-port fixtures and pending proof log; checks below | Product changes; low/S |
| ER32a / er32a-storage | Engine/Sol | — | T12 engine persistence/assets only; checks below | Snapshot, v1 removal, live callers; high/M |
| ER32b / er32b-snapshot | Snapshot/Terra | ER32a; snapshot lane free | Remaining T12: v2 snapshot/error state, derived revision and push integration; desktop `snapshot`, check-ui/test-ui | Editor/save behavior; med/S |
| ER22 / er22-parking-proof | Native/Sol | ER21 | T02 debug park variants and macOS scenarios below; desktop `browser`, docs-check | Product parking; high/S |
| ER23 / er23-world-proof | Native/Sol | ER21; lane free after ER22 | T03 isolated transport probe; macOS scenarios below; desktop `browser`, docs-check | Product runtime; high/M |
| ER24 / er24-editor-proof | Native/Sol | ER21; lane free after ER23 | T04 two candidate surface probes; macOS scenarios below; check-ui, desktop `browser` | Real editor/drafts; high/S |
| ER25 / er25-native-proof | Native/Sol | Passed ER22–24 | T05 navigation/snapshot/store proofs and next available ADR; desktop `browser`, docs-check | Product state, unproven mechanisms; high/M |
| ER26 / er26-identity | Identity/Sol | Passed ER25 | T06 persisted stable tab identity, registry, generations; engine suite, desktop `tabs::`/`browser` | Artifact relocation/navigation; high/M |
| ER27 / er27-navigation | Native/Sol | ER26, passed ER25 | T07 backend normalization/native state/history/Stop/error UI; desktop `browser`, check-ui/test-ui, macOS nav scenarios | Annotation matching; high/M |
| ER28 / er28-validation | Transport/Sol | ER26, ER27 | T08 bounded envelope, native sender/document validation and frontend decoder; desktop `browser`, check-ui/test-ui | Runtime/editor implementation; high/M |
| ER29 / er29-presentation | Presentation/Sol | ER26, passed ER22; native lane free | T09 sole presentation owner, leases, bounds/focus races; desktop `browser`, check-ui/test-ui, macOS preservation scenarios | Drafts/capture; high/M |
| ER30+31 / er30-31-runtime-drafts | Runtime/editor/Sol | ER28, ER29, ER32a/b, passed ER23/24; save contract resolved | T10+T11 isolated picking/locators plus trusted host editor/drafts/save, merged together; desktop `browser`, engine annotation filter, check-ui/test-ui, real-window trust/draft checks | Capture/export; high/L |
| ER33 / er33-capture | Capture/Sol | ER30+31, ER32a/b, passed ER25; marker contract resolved | T13 snapshot coordinator, crop, preview and invalidation; desktop `browser`, check-ui/test-ui, real-window scale/cancellation checks | Display capture/new adapters; high/M |
| ER34a / er34a-renderer | Engine/Sol | ER32a | T14 shared pure prompt renderer only; engine `browser_prompt` and `export`; checks below | Switching current export/Copy readers; high/S |
| ER34b / er34b-export | Export/Terra | ER34a, ER30+31; delivery contract resolved | Remaining T14 Copy/file/explicit agent text wiring; engine `export`, desktop `browser`, check-ui/test-ui | Adapter image support or automatic send; med/S |
| ER35 / er35-mcp | MCP/Sol | ER32a, ER34a | T15 MCP tool, same bucket resolution, bounded image delivery; MCP suite, checks below | Browser product/capture integration; high/S |
| ER36 / er36-cutover | Release/Sol | Every predecessor complete; all native acceptance passed | T16 replacement audit, deletion, platform checks, release notes; full gates below | New features; high/M |

Sizes mean S under one day, M roughly 1–3 days, L requires size review. Any PR
approaching 1,500 changed lines is split by independently reviewable responsibility
except the explicitly combined ER30+31 merge requirement. Estimates exclude owner
native testing and toolchain setup. A missing SDK or dependency is a reported
verification blocker, never a reason to mark a check passed.

## First implementation acceptance

ER21 owns only `tests/fixtures/browser/` and the new
`internal-docs/browser-feature-refactor/proof-log.md`. Serve 5173/3000 on localhost
and 127.0.0.1, print port/origin and fresh document IDs; implement T01 redirects,
slow/status/cookie routes, SPA/form/frames/open+closed-shadow/hostile/popup/dialog/
canvas/password/responsive/nested-scroll pages. Add actual keyboard listeners to
the hostile fixture so later editor isolation is observable. Keep all proofs
pending. `bun test tests/fixtures/browser/serve.test.ts`, the T01 curl redirect
check, and docs-check must pass; actual Safari/Tauri observations remain pending.

ER32a adds engine-only v2 types/load/save/delete/resolution and asset read APIs in
a new ai module; read T12 and reference spec's identity/save sections. Keep view
buckets supplied by callers, no new path convention. Missing metadata is empty;
corrupt/unsupported/invalid data fails and blocks every mutation. Preserve optional
v2 fields on rewrites. Serialize mutations per bucket; atomically rename assets
then metadata, remove newly written assets on metadata failure, never reuse a
deleted number, and return an existing record for repeated idempotency keys.
Use host-created IDs, bounded finite geometry/text/DOM, PNG validation/dimensions,
4 MiB and 4096-pixel limits, strict asset IDs, and containment checks including
symlinks. Do not accept caller paths. Page identity is structured evidence; query
normalization policy remains open before any live caller persists page evidence.
Keep capture-time geometry distinct from current resolution. No screenshot marker
number strategy is selected by this storage slice.

Run engine `browser_annotations`, `cargo fmt --all -- --check`, and
`just clippy` through the lock. Cover missing/corrupt/future-version metadata;
mutations refusing corrupt files; full round trip and unknown optional fields;
idempotency and deletion/monotonic numbering; concurrent saves; invalid/oversized/
malformed PNG and dimensions; traversal/symlink IDs; atomic-write failure cleanup.
Use deterministic failure injection, not permissions that may behave differently
as root. Also check `cargo check -p er-engine --no-default-features` through the
lock so headless consumers remain supported. ER32 is incomplete until ER32b.

ER34a preserves stored numbers and separates page evidence from reviewer text.
Collapse page-derived newlines and escape structure, including captions; test
hostile URLs/labels/selectors/DOM and nonconsecutive numbers. The renderer must
support truthful text-only/image-included/omitted statuses, never claim an image
is attached unless the caller delivers it. No existing reader switches yet.
ER35 adds `pr_browser_annotations` with the existing PR-reference fields, optional
bucket, include_images default true, text then caption/image pairs then v2 JSON.
Return metadata load errors; expose asset IDs, never internal absolute paths.
Cap aggregate asset bytes at 16 MiB and state omissions accurately. Test registration,
PR/local buckets, no-image mode, bytes/order, hostile captions, corrupt metadata,
missing assets and delete/read races: return a complete record/image or omit it
consistently. Existing MCP methods may still expose their established bucket fields;
this task must not expand their scope.

## Native owner actions and release acceptance

Owner runs these with the fixture server in the actual Tauri macOS window and
records date, macOS/hardware/Tauri/Wry versions and repeatable per-scenario results:

- ER22: hide/resign/detach variants; unchanged document/form/scroll/history,
  nested real modals, parked input rejection, 50 rapid cycles, native dialogs and
  window switching. Select only a repeatedly reliable variant; no timing retries.
- ER23: first-document and subsequent document-start installation, isolated
  globals/handler, genuine versus synthetic clicks, main-frame provenance,
  host/runtime round trip, shell IPC, 20 reloads without duplicates.
- ER24: child webview first, borderless child window if needed; typing/shortcuts
  invisible to page listeners, stacking after browser re-show, motion, Escape,
  focus return, transparency, resize and mixed display scale. Require all scenarios
  for the chosen surface; adjacent editor needs an owner UX decision if both fail.
- ER25: redirects/SPA/doc IDs, history/Stop, refused loads and HTTP errors with
  reliable attribution, cross-port navigation, native snapshot with target
  alignment within ±2 px, shell/editor exclusion, parked capture, 100%/125% zoom
  on Retina, persistent cookies across restart/tabs, and popup activation evidence.

A no/flaky/partial proof blocks its affected product behavior. Preserve unknown
results and ask for the specific mechanism/scope choice after useful independent
work; do not invent positive native evidence. No replacing Wry delegates. Keep
probe commands and adapters debug-only until their integration PRs.

ER36 requires every reference acceptance row recorded on macOS: preservation,
navigation/history, popup/session policy, hostile-message rejection, framed/shadow/
region targets and resolution, drafts/bucket/retry races, capture cancellation and
scale, storage corruption/assets/concurrency, prompt/MCP parity, and unsupported
platform action messages. Audit TUI and export readers before deleting shared v1
types. Delete old injection/composer/commands/caches/capabilities/proxy fallback
only after replacement acceptance. Leave old user files alone. Run sequentially
through the shared lock: `just lint`, `just test`, `just test-desktop`,
`just test-ui`, `cargo test -p er-mcp`; record macOS/Windows/Linux build evidence and
draft release notes before release review. A merged Linux-safe slice is progress,
not completion of ER22–31, ER33 or ER36.
