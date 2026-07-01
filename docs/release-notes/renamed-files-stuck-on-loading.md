# Renamed files no longer stick on "Loading content…" (desktop)

## What changed

In a large diff (desktop lazy-load mode), a file that was **purely renamed** — moved
with no content change (`+0 −0`) — used to sit on **"Loading content…"** forever. The
same happened to binary and mode-only changes. Now those files render immediately:

- A pure rename shows **"File renamed without changes."** (matching how GitHub presents
  a no-content rename).
- Binary / mode-only changes show **"No changes"** instead of a perpetual spinner.

## Why it's safe

Lazy mode marks a file as a "still-loading stub" when it has no parsed hunks. A pure
rename / binary / mode-only change has no `+`/`-` lines at all, so parsing it can never
produce hunks — it was being re-classified as a stub on every pass and never escaped the
loading state. The fix only narrows that classification to files that actually have
content to parse (`adds + dels > 0`), which is the **same** condition `ensure_file_parsed_at`
already uses to decide whether a file is worth fetching from git. No content file changes
behavior; only the genuinely-empty cases stop being treated as loadable.

## Implementation

- `crates/er-desktop/src/snapshot.rs` — `build_file_snapshot` only flags `is_lazy_stub`
  when `f.adds + f.dels > 0`, so renames / binary / mode-only files fall through to the
  normal no-changes render instead of a perpetual stub.
- `desktop-ui/src/lib/diffRenderModel.ts` / `NoChangesRow.svelte` — the no-changes render
  row carries a `renamed` flag (from the file's `renamed` status) so the UI can show
  "File renamed without changes." for renames and "No changes" otherwise.
- Tests cover the predicate (a lazy pure rename is not a stub; an unparsed content file
  still is) and the render row's rename flag.
