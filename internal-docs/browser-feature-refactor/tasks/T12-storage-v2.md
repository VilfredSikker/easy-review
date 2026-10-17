# T12 — Annotation storage v2

Status: ready-for-agent
Depends on: nothing (pure engine code; can run in parallel with T01–T05)
Size: 1–2 days
Risk: high (data integrity: review evidence on disk)

> **For the executor:** Build exactly this, in order. Each step's invariant is what
> must hold; if the code differs from what is cited, stop and ask.

## In plain terms

- **The problem.** Browser annotations are stored in `ui-annotations.json`. If that
  file is damaged, loading it silently returns an empty list, and the next save
  overwrites the damaged file — the evidence is gone. Screenshots are stored as
  absolute paths, one command reads any path it is given, and another accepts an
  unchecked ID in a file name.
- **The fix.** A new file, `browser-annotations.json`, with a version number,
  richer target data, stable comment numbers, and screenshots stored by ID inside
  the annotation folder. Loading a damaged or newer-version file is an error that
  blocks writes. The old file is ignored (decision: no migration — the only data
  is the owner's test data).
- **TL;DR.** A new annotation file that can never be silently wiped.

## Definition of done

- [ ] `crates/er-engine/src/ai/browser_annotations.rs` defines the v2 types, load,
      save, asset write/read and numbering, re-exported from `crates/er-engine/src/ai/mod.rs`.
- [ ] Load returns `Result<Vec<BrowserAnnotation>, BrowserAnnotationsError>`; a
      missing file is `Ok(vec![])`; malformed JSON, unknown version, or an invalid
      record is an `Err`, and every write path refuses to run after an `Err`.
- [ ] Saves are atomic (temp file + rename) and serialized per directory.
- [ ] Assets are stored as `<dir>/browser-assets/<asset_id>.png`, referenced by ID,
      validated (PNG signature, ≤ 4 MiB, ≤ 4096 px per side) before writing; reads
      take an asset ID, never a path.
- [ ] Numbers are assigned on first save, monotonic per file, never reused after
      deletion.
- [ ] Idempotent save: the same idempotency key returns the existing annotation.
- [ ] Unit tests in Step 6 pass.

## Before you start

- **Current v1 code** (`crates/er-engine/src/ai/comments.rs`):
  - `:417` `pub struct UiAnnotation` (url path only, selector, box, viewport,
    text, timestamp, author, `screenshot_path: Option<String>` absolute, stale,
    element_context, dom_context).
  - `:450` `load_ui_annotations` — returns `Vec`, maps parse errors to empty.
  - `:472` `save_ui_annotations` — writes `ui-annotations.json` via `.tmp` + rename.
- **Current consumers** (stay on v1 until the tasks named switch them):
  `crates/er-desktop/src/snapshot.rs:19` (`load_ui_annotations_cached`, mtime cache
  — copy this pattern for v2), `crates/er-desktop/src/commands.rs` annotation
  commands (from line 10210), `crates/er-engine/src/export.rs:208` (T14 switches it).
- **Storage location:** `TabState::comments_dir()`
  (`crates/er-engine/src/app/state/mod.rs:2314`) — the active view bucket under
  managed storage (ADR 0003, ADR 0004). Do not build paths from the repo root.
- **Fields:** `reference/spec.md`, "Annotation identity and geometry" and
  "Draft, capture, and save transaction".

## Scope

- **In scope:** the new engine module and its tests; a desktop read path that
  surfaces v2 annotations in the snapshot (new field, ADR 0034 revision).
- **Out of scope:** deleting v1 code (T16), the editor (T11), capture (T13).

## Steps

### Step 1 — Types

- **Do this:**
  ```rust
  pub const BROWSER_ANNOTATIONS_FILE: &str = "browser-annotations.json";
  pub const BROWSER_ANNOTATIONS_VERSION: u32 = 2;

  pub struct BrowserAnnotationsFile { pub version: u32, pub next_number: u32, pub annotations: Vec<BrowserAnnotation> }

  pub struct BrowserAnnotation {
      pub id: String,               // "ba-<hex>", host-allocated
      pub number: u32,              // shown on the marker and in prompts
      pub idempotency_key: String,
      pub created_at: String, pub author: String, pub text: String,
      pub page: PageIdentity,       // origin (with port), path, query (sensitive params removed), hash route
      pub locator: Option<Locator>, // None for region targets
      pub target: TargetDescription,// kind (element|region), role, name, text excerpt, limitation
      pub captured: CapturedGeometry,// viewport rect, viewport size, scroll, device scale, zoom
      pub dom_context: Option<serde_json::Value>, // ≤ 24 KiB
      pub resolution: Resolution,   // resolved|missing|ambiguous (last known)
      pub image: ImageEvidence,     // None | Captured { asset_id, crop, marker, captured_at } | Unavailable { reason }
  }
  ```
  `Locator { frames, shadow_hosts, selector, fingerprint }` matches T10.
  Derive `Serialize, Deserialize, Clone, Debug, PartialEq`. Do not use
  `deny_unknown_fields`: an added optional field must not break loading. An
  unknown `version` number must (Step 2).
- **Why / invariant:** `number` is assigned once and is the only number used for
  this annotation anywhere (marker, prompt heading, image caption).
- **Verify:** `~/.claude/scripts/locked.sh "$(git rev-parse --show-toplevel)" cargo check -p er-engine`
  passes; a round-trip test (serialize a full record, deserialize, assert equal) passes.

### Step 2 — Load

- **Do this:** read file; absent → `Ok(empty)`; parse to `serde_json::Value`,
  check `version == 2` else `Err(UnsupportedVersion(v))`; then deserialize; any
  failure → `Err(Malformed(msg))`. Validate each record (lengths, finite numbers)
  → `Err(InvalidRecord{id, reason})`.
- **Verify:** the Step 6 tests "missing file", "malformed JSON" and "version 3"
  pass (write them now).

### Step 3 — Save

- **Do this:** `save_browser_annotation(dir, new: NewBrowserAnnotation, image: Option<&[u8]>) -> Result<BrowserAnnotation, …>`:
  take a process-wide lock keyed by `dir`; load (abort on `Err`); if an annotation
  with the same idempotency key exists, return it; write the image asset first
  (validated, `browser-assets/<asset_id>.png` via temp + rename); assign
  `number = next_number`, increment; write the file via temp + rename; if the
  metadata write fails, delete the new asset and return the error.
  Also `delete_browser_annotation(dir, id)` (removes the record, then its asset)
  and `update_resolution(dir, id, resolution)`.
- **Watch out for:** never hold this lock while holding the desktop App mutex;
  callers do slow I/O outside the App lock (ADR 0015).
- **Verify:** the Step 6 tests for idempotency, numbering, metadata-failure
  cleanup and concurrent saves pass.

### Step 4 — Assets

- **Do this:** `read_browser_asset(dir, asset_id) -> Result<Vec<u8>>`. Validate
  `asset_id` matches `^ba-asset-[0-9a-f]{16,32}$`; build the path from `dir` only;
  canonicalize and check it is inside `dir/browser-assets`. No function takes a
  path from a caller.
- **Verify:** the Step 6 tests for oversized/non-PNG images and `../` asset IDs pass.

### Step 5 — Surface in the snapshot

- **Do this:** add `browser_annotations: Vec<…>` (or a load error marker) to the
  desktop snapshot, with an mtime cache like `load_ui_annotations_cached`, and add
  the file's mtime to the derived revision per ADR 0034. Mirror the type in
  `desktop-ui/src/lib/types.ts` and add a test that the Rust JSON shape matches
  the TS type (serialize a fixture in Rust, assert keys in a TS test), since types
  are hand-written in this repo.
- **Verify:** `cargo test -p er-desktop snapshot` and
  `cd desktop-ui && bun run test src/lib/snapshotChrome.test.ts` pass (through
  `locked.sh`); writing a valid `browser-annotations.json` by hand into a tab's
  bucket shows it in the snapshot without waiting for the 30 s timer.

### Step 6 — Tests

`./scripts/er-tui.sh test -p er-engine browser_annotations` (through `locked.sh`):
missing file → empty; malformed JSON → Err and save refuses; version 3 → Err;
save twice with one key → one record; numbers 1,2,3 then delete 2 then save →
4; image bigger than 4 MiB or not PNG → Err and no file written; metadata write
failure (make the directory read-only after asset write) → asset removed; asset
ID with `../` → Err; concurrent saves from 8 threads → 8 records, unique numbers.

## Rollback

Delete the module and snapshot field; v1 is untouched.

## When to stop and ask

- `comments_dir()` resolves to a repo-local path in your environment (it should
  not unless repo-local storage is explicitly on).
- A caller needs to read an asset by path.
