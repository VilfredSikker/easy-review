# T06 — Stable review-tab and browser identity

Status: ready-for-agent
Depends on: T05 (the ADR fixes the native design this identity serves)
Size: 2 days
Risk: high (tab persistence format changes; a mistake loses open tabs)

> **For the executor:** Build exactly this, in order. Preserve each step's
> invariant; if the code differs from what is cited, stop and ask.

## In plain terms

- **The problem.** Each review tab's browser is named after the tab's **position**
  (`review-browser-0`, `-1`, …). Close or reorder a tab and positions shift, so the
  app destroys every browser page to stay consistent, and messages from a page can
  be applied to whichever tab now sits at that position.
- **The fix.** Give each review tab a permanent ID that survives reorder, close of
  a sibling, and app restart. Key every browser by that ID, plus a generation
  number that changes whenever its native page is replaced, so late messages from
  an old page can be recognized and dropped.
- **TL;DR.** Name browsers by tab identity, not tab position.

## Definition of done

- [ ] `TabState` has `pub tab_id: String`, assigned once at creation, never reused.
- [ ] `TabDescriptor` persists it (`#[serde(default)]`); old `tabs.json` files load
      and get IDs assigned once, then saved.
- [ ] A backend `BrowserRegistry` maps `tab_id → BrowserEntry { browser_id, generation, label }`.
      Native labels are `review-browser-<browser_id>`; `browser_id` is stable for
      the tab's life.
- [ ] Reordering tabs and closing a sibling no longer call
      `reset_all_tab_webviews`; only the closed tab's browser is closed.
- [ ] Tests listed under "Verify" pass.

## Before you start

- **Tab state:** `crates/er-engine/src/app/state/mod.rs:653` — `pub struct TabState`.
  Browser fields at lines 1031–1042 (`browser_url`, `browser_layout`,
  `browser_split_ratio`, `browser_annotate_mode`, `browser_show_tooltips`).
  `TabState` literals are built in five places; find them with
  `grep -n "browser_show_tooltips: false" crates/er-engine/src/app/state/mod.rs`
  (lines 1656, 1789, 1918, 2059, 9173 at the time of writing).
- **Tab list operations:** `close_tab_at` (`mod.rs:6407`), `reorder_tabs` (`mod.rs:6442`).
- **Persistence:** `crates/er-desktop/src/tabs.rs:25` — `pub struct TabDescriptor`
  (all fields `#[serde(default)]` except `kind` and `repo_root`);
  `descriptor_from_tab` (`tabs.rs:142`), `apply_descriptor_browser` (`tabs.rs:132`),
  `rebuild_tab_with` (`tabs.rs:310`), test `round_trip_serializes_and_deserializes`
  (`tabs.rs:356`).
- **Commands that reset browsers today** (`crates/er-desktop/src/commands.rs`):
  `close_tab` (line 9491) calls `reset_all_tab_webviews` then `on_tab_selected`;
  `select_tab` (9509) calls `on_tab_selected`; `reorder_tabs` (10191) only persists.
- **Browser state:** `crates/er-desktop/src/browser_webview.rs:18` — `webview_label(tab_idx: usize)`;
  `:23` — `BrowserWebviewState { created: Mutex<HashMap<usize, bool>>, active_tab, annotate_by_tab }`.
- **Capability:** `crates/er-desktop/capabilities/browser-remote.json` lists labels
  `review-browser-0` … `review-browser-15`. New labels will not match it. Until
  T10 removes the capability, use a glob if Tauri's capability `webviews` field
  accepts one (`"review-browser-*"`); check the Tauri 2 capability docs and the
  schema referenced by the file's `$schema` key. If it does not, stop and ask.
- No ID crate is in `er-engine` or `er-desktop` `Cargo.toml`. Generate IDs as
  `format!("tab-{:x}-{:x}", unix_nanos, process_counter)` with an `AtomicU64`
  counter; do not add a dependency for this.

## Scope

- **In scope:** `TabState.tab_id`, `TabDescriptor.tab_id`, a new
  `BrowserRegistry` (in a new file `crates/er-desktop/src/browser_registry.rs`),
  `browser_webview.rs` keyed by `tab_id`, the three tab commands, the capability
  label pattern, tests.
- **Out of scope:** navigation behavior (T07), message routing (T08), modal
  handling (T09). Keep `on_tab_selected`'s navigate-on-select behavior for now;
  T07 removes it.

## Steps

### Step 1 — Add `tab_id` to `TabState`

- **Why / invariant:** the ID never changes for the life of a tab and is never
  reused by another tab. Artifact directories (`comments_dir()`, `mod.rs:2314`) must
  not depend on it — review data stays where it is.
- **Do this:** add the field after `browser_show_tooltips`; set it in all five
  literals with a new `pub fn new_tab_id() -> String` in the same module.
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" ./scripts/er-tui.sh test -p er-engine` → passes.

### Step 2 — Persist it

- **Do this:** `TabDescriptor { #[serde(default)] pub tab_id: Option<String>, … }`.
  `descriptor_from_tab` writes `Some(tab.tab_id.clone())`; the rebuild path sets
  `tab.tab_id` from the descriptor when present, otherwise keeps the freshly
  generated one. Persist after load so assigned IDs stick.
- **Verify:** extend `round_trip_serializes_and_deserializes` with a `tab_id`;
  add a test that a descriptor JSON **without** `tab_id` deserializes and gets an
  ID. Run `cargo test -p er-desktop tabs::` (through `locked.sh`).
- **If it fails:** an old file must always load. Never make `tab_id` required.

### Step 3 — Add the browser registry

- **Do this:** `BrowserRegistry { entries: Mutex<HashMap<String /*tab_id*/, BrowserEntry>> }`
  with `BrowserEntry { browser_id: String, generation: u64, label: String }`.
  Methods: `entry_for(tab_id) -> BrowserEntry` (create on first use), `bump_generation(tab_id) -> u64`
  (call whenever the native child is created or replaced), `remove(tab_id)`,
  `tab_for_label(label) -> Option<(String, u64)>`. Register it with `.manage(...)`
  in `crates/er-desktop/src/main.rs` beside `BrowserWebviewState`.
- **Why / invariant:** the label is derived from `browser_id`, never from a tab
  index. `tab_for_label` is how T08 will identify which tab a native message came from.
- **Verify:** unit tests in `browser_registry.rs`: generation increases; labels
  unique across 1000 entries; remove then re-add yields a new `browser_id`.

### Step 4 — Key `browser_webview.rs` by tab ID

- **Do this:** replace `usize` keys in `BrowserWebviewState` and every function in
  `browser_webview.rs` that takes `tab_idx` with `tab_id: &str`, resolving the
  label through the registry. Add `pub tab_id: String` to `TabSummary`
  (`crates/er-desktop/src/snapshot.rs:714`) and to its TS type in
  `desktop-ui/src/lib/types.ts`. Browser commands take `tabId: String` from the
  frontend instead of `tabIdx`; update every caller in
  `desktop-ui/src/lib/stores/browserHost.ts` (its `activeTabIdx` helper becomes an
  active-tab-ID helper reading the active `TabSummary`). An unknown `tabId` is an
  error, never a fallback to the active tab. `ensure_tab_webview` bumps the
  generation when it creates a child.
- **Why / invariant:** resolving an index at the command boundary can still pick
  the wrong tab if tabs are reordered while the call is in flight — the bug this
  task removes. Identity must travel from the frontend.
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" cargo check -p er-desktop`
  and `cd desktop-ui && bun run check` both pass; `grep -n "tabIdx" desktop-ui/src/lib/stores/browserHost.ts`
  returns nothing.
- **Watch out for:** `on_page_load` captures `tab_idx_capture` (around line 172);
  capture the tab ID instead, or a reordered tab's load events hit the wrong tab.

### Step 5 — Stop resetting every browser

- **Do this:** in `close_tab`, close only the closed tab's browser
  (`registry.remove` + close its label) — read the tab ID **before**
  `close_tab_at`. Leave `reorder_tabs` without any browser call.
- **Verify:** `cargo test -p er-desktop` (through `locked.sh`) passes. Then in the
  real app (`./scripts/tauri-dev.sh`): open three tabs with the browser on
  `form.html`, type text in tab 2, reorder tabs, close tab 1. **Expect:** tab 2's
  text and `#doc-id` unchanged.

### Step 6 — Capability labels

- **Do this:** make `browser-remote.json` match the new labels (glob or generated
  entries). Remove the 0–15 list.
- **Verify:** in the real app the page's annotation messages still arrive for a
  newly created tab whose label is not in the old list.

## Rollback

Revert the commits. Old `tabs.json` files still load because `tab_id` is optional.

## When to stop and ask

- The capability file cannot express dynamic labels.
- Any review artifact path changes as a result of this task.
