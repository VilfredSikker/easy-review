# Test Coverage Analysis — easy-review

## Current State: 324 tests, 9 of 24 files tested

The codebase has solid test foundations for core parsing and state logic, but significant
gaps exist in configuration, UI helper logic, and the comment/GitHub integration layer.

### Test Distribution

| Module | Lines | Tests | Coverage |
|--------|------:|------:|----------|
| `app/state.rs` | 5,533 | 101 | Good for navigation/scroll; gaps in comments, watched files |
| `ai/review.rs` | 2,153 | 63 | Good for queries; gaps in staleness, thread management |
| `git/diff.rs` | 1,222 | 39 | Excellent — parser well-covered |
| `app/filter.rs` | 483 | 31 | Excellent — model test coverage |
| `github.rs` | 931 | 26 | URL parsing solid; no tests for comment sync |
| `git/status.rs` | 985 | 18 | Log/shortstat parsing only; base branch detection untested |
| `main.rs` | 1,553 | 15 | Key-binding routing only |
| `ai/relocate.rs` | 556 | 9 | Decent coverage of relocation algorithm |
| `ai/loader.rs` | 308 | 9 | Hash computation only; file loading untested |
| `ui/file_tree.rs` | 589 | 8 | Path shortening only |
| `ui/utils.rs` | 108 | 5 | Word-wrap only |
| **config.rs** | **361** | **0** | **No tests** |
| **ui/diff_view.rs** | **1,939** | **0** | **No tests** |
| **ui/panel.rs** | **852** | **0** | **No tests** |
| **ui/status_bar.rs** | **842** | **0** | **No tests** |
| **ui/overlay.rs** | **338** | **0** | **No tests** |
| **ui/settings.rs** | **234** | **0** | **No tests** |
| **ui/styles.rs** | **207** | **0** | **No tests** |
| **ui/highlight.rs** | **139** | **0** | **No tests** |
| **watch/mod.rs** | **85** | **0** | **No tests** |

---

## Priority 1 — Critical Gaps (high value, pure logic, easy to test)

### 1. `config.rs` — 0 tests, 361 lines

This module has zero tests despite containing important pure logic and several
documented risk items.

**What to test:**

- **`deep_merge()`** — Pure function that recursively merges TOML tables. This is the
  most important untested function in the file. Test cases:
  - Empty base gets fully overwritten by overlay
  - Empty overlay preserves base
  - Scalar values get replaced (not merged)
  - Nested tables merge recursively (3+ levels deep)
  - Array values get replaced, not appended
  - Type mismatch (scalar overlaying table and vice versa)

- **`load_config()` deserialization** — Currently, parse errors in `.er-config.toml` are
  silently ignored (`TODO(risk:medium)` at line 183). Deserialization errors also silently
  fall back to defaults (line 204). Test that:
  - Missing config files produce correct defaults
  - Partial TOML (e.g., only `[features]` section) merges correctly with defaults
  - Local config overrides global config at the field level
  - Malformed TOML gracefully falls back (intentional behavior, but should be tested)

- **Default values** — Every `FeatureFlags` field defaults to `true`, `tab_width` to 4,
  agent command to `"claude"`. These are serde-driven and a single typo breaks them.
  Quick round-trip tests catch regressions.

- **`settings_items()`** — Pure function returning 27 UI items. Test that get/set closures
  on `BoolToggle` items actually read/write the correct `ErConfig` fields. A broken
  closure silently corrupts config.

### 2. `app/state.rs` — Comment system (0 tests for ~500 lines of comment logic)

The state file has 101 tests, but they're concentrated on navigation and scrolling.
The entire comment lifecycle is untested:

- **`submit_comment()`** — Creates questions or GitHub comments, handles replies, writes
  JSON. No test coverage. Test:
  - Creating a new question sets correct file/hunk/line fields
  - Creating a reply sets `in_reply_to` correctly
  - Editing an existing comment preserves its ID and updates text
  - Questions go to `.er-questions.json`, GitHub comments to `.er-github-comments.json`

- **`confirm_delete_comment()`** — Deletes a comment and cascades to its replies.
  Test that:
  - Deleting a parent comment also removes all replies
  - Deleting a reply leaves the parent intact
  - Focus moves to a valid comment after deletion

- **Comment focus navigation** (`next_comment()`, `prev_comment()`) — These traverse
  comments in file/hunk order. Test boundary conditions:
  - Single comment: next/prev stays in place
  - Comments spanning multiple files: navigation crosses file boundaries
  - Empty comment list: no crash

- **`start_comment()` / `start_reply_comment()` / `start_edit_comment()`** — These
  set up input mode state. Test that input mode transitions are correct.

### 3. `app/state.rs` — Visible files pipeline (partially tested)

The 3-phase visibility pipeline (filter rules → search → unreviewed toggle) has 7 tests,
but they don't cover the interaction between all three phases simultaneously. Missing:

- Filter rules + search query + unreviewed toggle all active at once
- `snap_to_visible_selected_file()` when all files are filtered out
- `apply_filter_expr()` history deduplication (capped at 20 entries)

### 4. `git/status.rs` — Base branch detection (0 tests for core logic)

`detect_base_branch_impl()` has a multi-step fallback chain (upstream tracking → main →
master → develop → dev) that is completely untested. This is one of the most critical
functions — it determines which diff the user sees by default.

**What to test (requires git repo fixtures):**
- Repo with upstream tracking branch → uses upstream
- Repo with only `main` → detects main
- Repo with only `master` → falls back to master
- Empty repo with no commits → `TODO(risk:medium)` at line 146, fragile behavior
- `strip_upstream_remote()` — pure function, already has 3 tests but could use more
  edge cases (no `/`, multiple `/` separators)

---

## Priority 2 — High Value Gaps

### 5. `github.rs` — Comment sync (0 tests for ~400 lines)

URL parsing is well-tested (26 tests), but the entire comment sync system
(`gh_pr_comments()`, `gh_pr_push_comment()`, `gh_pr_reply_comment()`,
`gh_pr_delete_comment()`) has zero tests.

These functions shell out to `gh` CLI, making them harder to unit test, but the JSON
parsing and deduplication logic within them is testable:

- **`gh_pr_comments()` JSON parsing** — Parses paginated GitHub API output with a manual
  bracket-depth parser (`TODO(risk:medium)` at line 378). Test with fixture JSON.
- **Comment deduplication** — On pull, comments are deduplicated by `github_id`.
  Test that duplicate IDs are merged correctly.
- **`verify_remote_matches()`** — Pure string matching of remote URLs against PR owner/repo.
  Already has tests for HTTPS/SSH patterns, but could test more edge cases.

### 6. `ai/review.rs` — Per-comment staleness (partially tested)

The staleness system (comments store `line_content` and go stale when diff changes) is
runtime-only and has no direct tests. Test that:

- A comment whose `line_content` no longer matches the diff line is marked stale
- A comment whose `line_content` still matches is not marked stale
- Staleness is recalculated on diff refresh

### 7. `ai/loader.rs` — `compute_per_file_hashes()` rename handling

Has 9 tests for basic hash computation, but `TODO(risk:medium)` at line 44 notes that
the parser assumes `a/` and `b/` paths are identical. For renamed files, this fails to
find the new name and never marks the file as stale. Test with a diff fixture that
includes a rename.

---

## Priority 3 — UI Pure Logic (moderate value)

These files are primarily rendering code, but contain embedded pure logic worth testing:

### 8. `ui/status_bar.rs` — `pack_hint_lines()` (lines 584-631)

A line-packing algorithm that wraps keyboard hints to fit terminal width. Pure function,
easy to test:
- Hints that fit in one line → 1 line returned
- Hints that exceed width → wrapped to multiple lines
- `bottom_bar_height()` matches actual packed line count

### 9. `ui/diff_view.rs` — Size formatting (lines 1757-1763)

Converts bytes to human-readable format (KB, MB). Pure function, trivial to test:
- 0 bytes, 1023 bytes (stays in bytes)
- 1024 bytes → "1.0 KB"
- 1,048,576 bytes → "1.0 MB"

### 10. `ui/panel.rs` — `check_icon()` and `review_state_style()`

Small pure functions mapping enums to display strings and colors. Low effort, prevent
regressions when adding new enum variants:
- `check_icon("success")` → checkmark
- `review_state_style("APPROVED")` → green label

### 11. `ui/overlay.rs` & `ui/settings.rs` — `centered_rect()`

Identical pure geometry function duplicated in two files. Test it once, and consider
deduplicating into `ui/utils.rs`:
- Centering a 50x10 popup in a 100x20 area
- Popup larger than area (clamping behavior)
- Zero-size edge case

---

## Priority 4 — Lower Value / Harder to Test

### 12. `main.rs` — Input handler dispatch

The 15 existing tests cover key-binding routing (Ctrl+Q vs bare Q, etc.). Additional
tests could cover:
- CLI argument validation (`--pr` with URL, `--filter` flag parsing)
- But these require terminal mocking — lower ROI

### 13. `watch/mod.rs` — File watcher

OS-dependent, uses `notify` crate with debouncing. Hard to unit test deterministically.
`TODO(risk:minor)` notes that watcher errors are silently discarded (line 39).
Integration testing would be more appropriate than unit tests here.

### 14. `ui/highlight.rs` — Syntax highlighting cache

Cache eviction logic (10K entry limit) could be tested, but it's simple threshold logic
not worth the effort unless bugs appear.

---

## Summary: Recommended Test Additions by Effort

| Priority | Area | Est. Tests | Effort |
|----------|------|-----------|--------|
| **P1** | config.rs (deep_merge, defaults, serde) | ~20 | Low |
| **P1** | app/state.rs comment lifecycle | ~15 | Medium |
| **P1** | app/state.rs filter pipeline interactions | ~5 | Low |
| **P1** | git/status.rs base branch detection | ~8 | Medium (needs git fixtures) |
| **P2** | github.rs comment JSON parsing | ~10 | Medium (fixture-based) |
| **P2** | ai/review.rs staleness detection | ~5 | Low |
| **P2** | ai/loader.rs rename handling | ~3 | Low |
| **P3** | ui/status_bar.rs hint packing | ~5 | Low |
| **P3** | ui/diff_view.rs size formatting | ~3 | Low |
| **P3** | ui/panel.rs helper functions | ~5 | Low |
| **P3** | ui/overlay.rs centered_rect | ~3 | Low |
| **Total** | | **~82** | |

The P1 items alone (~48 tests) would significantly improve confidence in the areas most
likely to regress: configuration loading, the comment system, filter interactions, and
base branch detection.

---

## Structural Recommendations

1. **Add integration tests** — Create a `tests/` directory with end-to-end tests that
   set up a real git repo (via `tempdir` + `git init`), write files, and exercise the
   full diff-parse-filter-display pipeline.

2. **Consider property-based testing** — The diff parser (`git/diff.rs`) and filter
   system (`app/filter.rs`) are pure functions operating on structured text. Tools like
   `proptest` or `quickcheck` could find edge cases the hand-written tests miss (e.g.,
   CRLF handling, Unicode in paths).

3. **Extract testable logic from rendering code** — Functions like `pack_hint_lines()`,
   `centered_rect()`, `check_icon()`, and size formatting are pure logic embedded in
   rendering modules. Extracting them to utility modules (or at minimum adding `#[cfg(test)]`
   blocks) would make them testable without any rendering framework.

4. **Address the duplicated `centered_rect()`** — This function appears identically in
   both `ui/overlay.rs` and `ui/settings.rs`. Move it to `ui/utils.rs` and test once.

5. **Test the documented risk items** — There are 50+ `TODO(risk:*)` annotations in the
   code. Many describe silent failure modes (config parse errors silently ignored, write
   errors discarded, hash collisions). Tests that explicitly exercise these paths would
   catch regressions if the "silent" behavior is ever accidentally removed or changed.
