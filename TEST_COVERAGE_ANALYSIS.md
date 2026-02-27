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

## Checklist

Track progress by checking items off as tests are added.

### P1 — Critical Gaps

#### config.rs (~20 tests)
- [ ] `deep_merge()` — empty base overwritten by overlay
- [ ] `deep_merge()` — empty overlay preserves base
- [ ] `deep_merge()` — scalar values replaced, not merged
- [ ] `deep_merge()` — nested tables merge recursively (3+ levels)
- [ ] `deep_merge()` — array values replaced, not appended
- [ ] `deep_merge()` — type mismatch (scalar overlaying table and vice versa)
- [ ] `load_config()` — missing config files produce correct defaults
- [ ] `load_config()` — partial TOML merges correctly with defaults
- [ ] `load_config()` — local config overrides global at field level
- [ ] `load_config()` — malformed TOML gracefully falls back to defaults
- [ ] Default values — `FeatureFlags` fields all default to `true`
- [ ] Default values — `tab_width` defaults to 4
- [ ] Default values — agent command defaults to `"claude"`
- [ ] Default values — serde round-trip preserves all fields
- [ ] `settings_items()` — returns expected number of items
- [ ] `settings_items()` — BoolToggle get/set closures read/write correct fields
- [ ] `settings_items()` — section headers present in correct order

#### app/state.rs — Comment lifecycle (~15 tests)
- [ ] `submit_comment()` — new question sets correct file/hunk/line fields
- [ ] `submit_comment()` — new GitHub comment sets correct fields
- [ ] `submit_comment()` — reply sets `in_reply_to` correctly
- [ ] `submit_comment()` — editing preserves ID and updates text
- [ ] `submit_comment()` — questions write to `.er-questions.json`
- [ ] `submit_comment()` — GitHub comments write to `.er-github-comments.json`
- [ ] `confirm_delete_comment()` — deleting parent cascades to replies
- [ ] `confirm_delete_comment()` — deleting reply leaves parent intact
- [ ] `confirm_delete_comment()` — focus moves to valid comment after deletion
- [ ] `next_comment()` / `prev_comment()` — single comment stays in place
- [ ] `next_comment()` / `prev_comment()` — crosses file boundaries
- [ ] `next_comment()` / `prev_comment()` — empty list no crash
- [ ] `start_comment()` — sets input mode correctly
- [ ] `start_reply_comment()` — sets reply target correctly
- [ ] `start_edit_comment()` — populates input buffer with existing text

#### app/state.rs — Filter pipeline (~5 tests)
- [ ] Filter rules + search query + unreviewed toggle all active simultaneously
- [ ] `snap_to_visible_selected_file()` when all files filtered out
- [ ] `apply_filter_expr()` history deduplication
- [ ] `apply_filter_expr()` history capped at 20 entries
- [ ] Filter cleared restores full file list

#### git/status.rs — Base branch detection (~8 tests)
- [ ] Repo with upstream tracking branch uses upstream
- [ ] Repo with only `main` detects main
- [ ] Repo with only `master` falls back to master
- [ ] Repo with `develop` branch detected in fallback chain
- [ ] Empty repo with no commits handles gracefully
- [ ] `strip_upstream_remote()` — no `/` in input
- [ ] `strip_upstream_remote()` — multiple `/` separators
- [ ] Branch on its own base (current == detected base) handled

### P2 — High Value Gaps

#### github.rs — Comment sync (~10 tests)
- [ ] `gh_pr_comments()` — parses valid GitHub API JSON fixture
- [ ] `gh_pr_comments()` — handles empty response
- [ ] `gh_pr_comments()` — paginated bracket-depth parser handles multi-page
- [ ] Comment deduplication by `github_id` on pull
- [ ] `verify_remote_matches()` — additional edge cases beyond existing tests
- [ ] `gh_pr_push_comment()` — constructs correct CLI arguments
- [ ] `gh_pr_reply_comment()` — sets correct thread ID
- [ ] `gh_pr_delete_comment()` — targets correct comment ID
- [ ] `gh_pr_overview()` — parses full PR metadata JSON
- [ ] `gh_pr_overview()` — handles missing optional fields gracefully

#### ai/review.rs — Staleness detection (~5 tests)
- [ ] Comment with matching `line_content` is not stale
- [ ] Comment with mismatched `line_content` is marked stale
- [ ] Staleness recalculated on diff refresh
- [ ] File-level staleness via diff hash comparison
- [ ] Mixed stale/fresh comments in same file

#### ai/loader.rs — Rename handling (~3 tests)
- [ ] `compute_per_file_hashes()` with renamed file produces correct path key
- [ ] Renamed file detected as stale when content changes
- [ ] `load_ai_state()` handles missing `.er-*` files gracefully

### P3 — UI Pure Logic

#### ui/status_bar.rs (~5 tests)
- [ ] `pack_hint_lines()` — hints fit in one line
- [ ] `pack_hint_lines()` — hints wrap to multiple lines
- [ ] `bottom_bar_height()` matches actual packed line count
- [ ] `spans_width()` — correct character counting
- [ ] `top_bar_height()` — single tab vs multi-tab

#### ui/diff_view.rs (~3 tests)
- [ ] Size formatting — bytes range (0, 1023)
- [ ] Size formatting — KB range (1024 → "1.0 KB")
- [ ] Size formatting — MB range (1048576 → "1.0 MB")

#### ui/panel.rs (~5 tests)
- [ ] `check_icon()` — maps all conclusion states correctly
- [ ] `review_state_style()` — maps all review states correctly
- [ ] File risk sorting — High before Medium before Low
- [ ] Comment target label formatting (hunk-only, hunk+line, file-level)
- [ ] Reviewer deduplication and sort order

#### ui/overlay.rs — centered_rect (~3 tests)
- [ ] Centering popup in larger area
- [ ] Popup larger than area (clamping)
- [ ] Zero-size edge case
- [ ] Deduplicate `centered_rect()` from `ui/settings.rs` to `ui/utils.rs`

### P4 — Lower Priority

#### main.rs (~3 tests)
- [ ] CLI argument validation — `--pr` with URL
- [ ] CLI argument validation — `--filter` flag
- [ ] CLI argument validation — conflicting arguments

#### Structural improvements
- [ ] Create `tests/` directory with integration tests (tempdir + git init)
- [ ] Consider `proptest`/`quickcheck` for diff parser and filter system
- [ ] Extract pure logic from rendering code into testable utilities
- [ ] Add tests for documented `TODO(risk:*)` silent failure modes
