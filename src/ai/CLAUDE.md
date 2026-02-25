# ai/ — AI Review Integration

Data model and loader for AI-generated review files. The `.er-*` files are produced by external tools (Claude Code skills) and consumed here. This module does NOT run AI — it reads AI output.

## Files

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | ~6 | Re-exports |
| `review.rs` | ~920 | Data model: all structs, enums, navigation, state management |
| `loader.rs` | ~145 | File I/O: loads `.er-*` JSON files, computes diff hash |

## External Files (written by AI skills, read here)

| File | Struct | Purpose |
|------|--------|---------|
| `.er-review.json` | `ErReview` | Per-file risk levels, findings, suggestions |
| `.er-order.json` | `ErOrder` | Suggested file review order with groupings |
| `.er-summary.md` | (raw text) | Markdown summary of overall changes |
| `.er-checklist.json` | `ErChecklist` | Review checklist items |
| `.er-feedback.json` | `ErFeedback` | Human comments (this is the only file `er` writes to) |

## Key Types (review.rs)

**`AiState`** — Aggregate state for one tab. Holds all five data types (optional) plus:
- `is_stale` — true if any `.er-*` file's `diff_hash` differs from current diff
- `view_mode: ViewMode` — `Default | Overlay | SidePanel | AiReview`
- `review_focus: ReviewFocus` — `Files | Checklist` (which AiReview column has focus)
- `review_cursor: usize` — cursor position within the focused column

**`ErReview`** → `ErFileReview` → `Finding` — hierarchical: review contains per-file reviews, each containing findings with severity, category, description, suggestion, hunk references.

**`RiskLevel`** — `High | Medium | Low | Info` with display helpers.

**`ViewMode`** — cycles via `v`/`V` keys. `Overlay` and `SidePanel` require AI data; `AiReview` requires review data specifically.

## Loader (loader.rs)

- `compute_diff_hash(raw_diff)` — SHA-256 hex string via `sha2` crate
- `load_ai_state(repo_root, current_diff_hash)` — reads all five files, sets `is_stale`
- `latest_er_mtime(repo_root)` — max mtime across all files; used for live-reload polling

## Important Patterns

- Staleness is all-or-nothing: if any `.er-*` file has a different `diff_hash`, the entire AI state is marked stale
- `AiState` preserves `view_mode/review_focus/review_cursor` across reloads (handled by `TabState::reload_ai_state()`)
- `ErFeedback` is the only file `er` writes to — all others are read-only from `er`'s perspective
- Finding banners link to hunks via `hunk_index: Option<usize>`, enabling inline display in the diff view
