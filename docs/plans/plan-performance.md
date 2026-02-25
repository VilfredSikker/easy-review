# Performance & Large Diff Handling

## Problem

A PR touching 500+ files and 10,000+ changed lines will stress every layer: git output loading, diff parsing, file tree rendering, diff view rendering, and AI overlay. Lock files, generated code, and binary diffs add noise without review value. This plan addresses both smart compaction and general performance hardening.

---

## 1. Auto-Compaction of Low-Value Files

Certain file types produce massive diffs that nobody reviews line-by-line. Compact them automatically.

### Compactable file patterns

```toml
# .er-config.toml
[compaction]
enabled = true

# Glob patterns — matched against file paths
patterns = [
  "*.lock",
  "package-lock.json",
  "yarn.lock",
  "pnpm-lock.yaml",
  "Cargo.lock",
  "Gemfile.lock",
  "poetry.lock",
  "composer.lock",
  "go.sum",
  "*.min.js",
  "*.min.css",
  "*.generated.*",
  "*.snap",            # jest snapshots
  "*.pb.go",           # protobuf generated
  "*.g.dart",          # code generation
  "__generated__/**",
  "migrations/*.sql",  # auto-generated migrations
]

# Size threshold — auto-compact any single file diff above this
max_lines_before_compact = 500
```

### Compacted file behavior

A compacted file shows a single collapsed line in the diff view instead of full hunks:

```
 📦 package-lock.json  +1,842 −1,201  (compacted — press Enter to expand)
```

### Data model

```rust
#[derive(Debug, Clone)]
pub struct DiffFile {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<DiffHunk>,
    pub adds: usize,
    pub dels: usize,

    // ── New ──
    /// Whether this file is compacted (hunks not parsed/rendered)
    pub compacted: bool,
    /// Raw hunk count before compaction (for display)
    pub raw_hunk_count: usize,
}
```

### Compaction during parse

Apply compaction in `parse_diff()` or immediately after:

```rust
pub fn compact_files(files: &mut Vec<DiffFile>, config: &CompactionConfig) {
    for file in files.iter_mut() {
        let total_lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();

        let should_compact = config.patterns.iter().any(|p| glob_match(p, &file.path))
            || total_lines > config.max_lines_before_compact;

        if should_compact {
            file.compacted = true;
            file.raw_hunk_count = file.hunks.len();
            file.hunks.clear();  // ← Free the memory
            file.hunks.shrink_to_fit();
        }
    }
}
```

This is the single biggest memory win — a 2,000-line lock file diff drops from ~200 KB of parsed structs to ~100 bytes.

### Expand on demand

When the user presses `Enter` on a compacted file, re-parse just that file's section from the raw diff. This requires either:

- **Option A:** Keep the raw diff string in memory and store byte offsets per file during initial parse
- **Option B:** Re-run `git diff -- <path>` for just that file (simpler, ~50ms)

Option B is simpler and avoids keeping the full raw diff in memory. Store nothing extra — just shell out on expand.

```rust
pub fn expand_compacted_file(file: &mut DiffFile, repo_root: &str, mode: &str, base: &str) -> Result<()> {
    let raw = git_diff_raw_file(mode, base, repo_root, &file.path)?;
    let parsed = parse_diff(&raw);
    if let Some(f) = parsed.into_iter().next() {
        file.hunks = f.hunks;
        file.compacted = false;
    }
    Ok(())
}

/// Run git diff for a single file
fn git_diff_raw_file(mode: &str, base: &str, repo_root: &str, path: &str) -> Result<String> {
    // git diff <base>..HEAD -- <path>
    // or git diff (unstaged) -- <path>
    // etc based on mode
}
```

### Keybindings

| Key | Context | Action |
|-----|---------|--------|
| `Enter` | On compacted file | Expand (parse and show hunks) |
| `Enter` | On expanded file (previously compacted) | Re-compact |

### File tree indicator

Compacted files show a `📦` icon and dimmed styling in the file tree. Their add/del counts are still visible but the file is marked as low-priority.

---

## 2. Virtualized Diff Rendering

**Current problem:** `diff_view.rs` builds a `Vec<Line>` for ALL lines of the selected file, even if only 40 are visible on screen. A 5,000-line file allocates 5,000 `Line` objects per frame.

### Viewport-based rendering

Only build `Line` objects for the visible window plus a small buffer:

```rust
pub fn render(f: &mut Frame, area: Rect, app: &App, hl: &Highlighter) {
    let tab = app.tab();
    let viewport_height = area.height as usize;
    let buffer_lines = 20;  // Pre-render 20 lines above/below viewport

    let scroll = tab.diff_scroll as usize;
    let render_start = scroll.saturating_sub(buffer_lines);
    let render_end = scroll + viewport_height + buffer_lines;

    let mut lines: Vec<Line> = Vec::with_capacity(viewport_height + buffer_lines * 2);
    let mut logical_line = 0;

    for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
        // Hunk header
        if logical_line >= render_start && logical_line < render_end {
            lines.push(render_hunk_header(hunk, hunk_idx));
        }
        logical_line += 1;

        // Hunk lines
        for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
            if logical_line >= render_start && logical_line < render_end {
                lines.push(render_diff_line(diff_line, hl, ...));
            }
            logical_line += 1;

            // Skip rest if past viewport
            if logical_line > render_end + buffer_lines {
                break;  // Early exit — don't process remaining lines
            }
        }

        // Comments, findings, etc. — same pattern
    }

    let visible_scroll = (scroll - render_start) as u16;
    let paragraph = Paragraph::new(lines).scroll((visible_scroll, tab.h_scroll));
    f.render_widget(paragraph, area);
}
```

### Pre-computed line index

To avoid iterating all hunks just to find the viewport window, build a lightweight index on file selection:

```rust
/// Precomputed index: maps logical line number → (hunk_idx, line_idx_in_hunk)
pub struct DiffLineIndex {
    /// Total logical lines for this file (hunks headers + diff lines + comments + findings)
    pub total_lines: usize,
    /// Start logical line for each hunk
    pub hunk_starts: Vec<usize>,
}
```

Built once when a file is selected (`select_file()`). Invalidated on refresh. Enables O(1) lookup of which hunk to start rendering from.

### Syntax highlighting cache

Highlighting the same line multiple times across frames is wasteful. Cache highlighted spans keyed by content hash:

```rust
use std::collections::HashMap;

pub struct Highlighter {
    // existing fields...
    cache: HashMap<u64, Vec<Span<'static>>>,  // content hash → spans
}
```

Use a fast hash (FxHash or xxhash) on line content. Cache hit rate will be very high since most lines don't change between frames.

---

## 3. Lazy Diff Parsing

**Current problem:** `parse_diff()` parses the entire raw diff (all files, all hunks, all lines) into memory before returning. For 10,000 lines this is ~1 MB of structs plus the raw string.

### Two-phase parse

**Phase 1: Header-only parse** — Fast scan that extracts file paths, status, add/del counts, and byte offsets. No line-level parsing.

```rust
pub struct DiffFileHeader {
    pub path: String,
    pub status: FileStatus,
    pub adds: usize,
    pub dels: usize,
    pub hunk_count: usize,
    pub byte_offset: usize,   // Start position in raw diff
    pub byte_length: usize,   // Length of this file's section
}

/// Fast scan — only reads diff headers, not line content
pub fn parse_diff_headers(raw: &str) -> Vec<DiffFileHeader> {
    // Look for "diff --git" lines and "@@" hunk headers
    // Count +/- lines without allocating Strings for each
    // ~10x faster than full parse for large diffs
}
```

**Phase 2: On-demand file parse** — When the user selects a file, parse just that file's hunks from the raw diff using the stored byte offset.

```rust
pub fn parse_file_hunks(raw: &str, header: &DiffFileHeader) -> Vec<DiffHunk> {
    let section = &raw[header.byte_offset..header.byte_offset + header.byte_length];
    // Parse only this file's hunks
}
```

### Memory model change

```rust
pub struct TabState {
    /// Lightweight headers for all files (always in memory)
    pub file_headers: Vec<DiffFileHeader>,

    /// Full parsed hunks — only for the currently selected file
    pub active_file: Option<ParsedFile>,

    /// Raw diff string (kept for on-demand parsing)
    raw_diff: String,
}

pub struct ParsedFile {
    pub index: usize,
    pub hunks: Vec<DiffHunk>,
}
```

Trade-off: keeps the raw diff string in memory (~200 KB for 10K lines, much less than parsed structs for all files). Could also drop the raw string and re-run `git diff -- <path>` on file selection, at the cost of ~50ms latency per file switch.

### When to full-parse vs lazy-parse

| Diff size | Strategy |
|-----------|----------|
| < 100 files, < 5,000 lines | Full parse (current behavior, fast enough) |
| 100–500 files or 5,000–20,000 lines | Header-only + on-demand file parse |
| > 500 files or > 20,000 lines | Header-only + compaction + on-demand parse |

Detection happens in `refresh_diff()`:

```rust
let line_count = raw.lines().count();  // Quick O(n) scan
let strategy = if line_count > 5_000 || file_count > 100 {
    ParseStrategy::Lazy
} else {
    ParseStrategy::Eager
};
```

---

## 4. File Tree Performance

**Current problem:** `visible_files()` collects all files into a Vec and does HashMap lookups per file, every frame.

### Cached visible files

Only recompute `visible_files()` when inputs change (search query, filter toggle, file list):

```rust
pub struct FileTreeCache {
    /// Cached visible file indices
    visible: Vec<usize>,
    /// Inputs that produced this cache
    search_query: String,
    show_unreviewed_only: bool,
    file_count: usize,
}
```

Invalidate on search input change, filter toggle, or diff refresh. Saves 500+ HashMap lookups per frame during normal navigation.

### Virtualized list rendering

Same principle as diff rendering — only build `ListItem` objects for visible rows:

```rust
let viewport_start = tab.file_scroll as usize;
let viewport_end = viewport_start + area.height as usize;

let items: Vec<ListItem> = cached_visible[viewport_start..viewport_end.min(cached_visible.len())]
    .iter()
    .map(|&idx| build_list_item(&tab.files[idx], ...))
    .collect();
```

With 500 files but a 40-row terminal, this builds 40 ListItems instead of 500.

---

## 5. Diff Refresh Optimization

**Current problem:** `refresh_diff()` can invoke `git diff` 2–3 times per refresh (current mode + branch mode + staleness check).

### Deduplicate git calls

```rust
fn refresh_diff_impl(&mut self, recompute_branch_hash: bool) -> Result<()> {
    // Always need current mode diff
    let raw = git::git_diff_raw(self.mode.git_mode(), &self.base_branch, &self.repo_root)?;
    self.diff_hash = ai::compute_diff_hash(&raw);

    // Branch diff — only if needed AND mode is not already branch
    let branch_raw = if self.mode == DiffMode::Branch {
        self.branch_diff_hash = self.diff_hash.clone();
        Some(raw.clone())  // Reuse — no second git call
    } else if recompute_branch_hash {
        let br = git::git_diff_raw("branch", &self.base_branch, &self.repo_root)?;
        self.branch_diff_hash = ai::compute_diff_hash(&br);
        Some(br)
    } else {
        None
    };

    // Staleness — reuse branch_raw if available
    if self.ai.is_stale {
        if let Some(ref br) = branch_raw {
            self.compute_stale_files(br);
        }
    }

    // Parse current diff
    self.files = git::parse_diff(&raw);
    // ... rest
}
```

This guarantees at most 2 git diff calls (down from 3).

### Debounce file watcher refreshes

Multiple rapid file changes (e.g., `git checkout`) fire many watch events. Debounce to avoid redundant refreshes:

```rust
// In event loop
if file_changed {
    self.pending_refresh = true;
    self.refresh_deadline = Instant::now() + Duration::from_millis(200);
}

if self.pending_refresh && Instant::now() >= self.refresh_deadline {
    self.pending_refresh = false;
    tab.refresh_diff_quick()?;
}
```

### Incremental diff hash

Instead of hashing the entire raw diff string, compute a fast hash:

```rust
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

pub fn compute_diff_hash_fast(raw: &str) -> String {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
```

This is much faster than SHA-256 for change detection. Only use SHA-256 for `.er-review.json` compatibility where the hash is persisted.

---

## 6. Large File Warnings

For files exceeding a threshold, show a warning before rendering:

```
  ⚠ src/generated/api_types.rs  +3,842 lines
  Large file — press Enter to load, or skip with ↓
```

### Threshold

```toml
# .er-config.toml
[performance]
large_file_warning_lines = 2000
```

Files above this threshold are not parsed until explicitly opened. Similar to compaction but driven by size rather than pattern.

---

## 7. Memory Budget Tracking

Add lightweight memory tracking to detect when the tool is under pressure:

```rust
pub struct MemoryBudget {
    pub parsed_files: usize,   // Number of fully parsed files in memory
    pub total_lines: usize,    // Total DiffLine objects alive
    pub render_lines: usize,   // Lines in last render pass
}
```

Log warnings when thresholds are exceeded:

```rust
if budget.total_lines > 50_000 {
    log::warn!("High memory: {} parsed lines in memory", budget.total_lines);
}
```

Show in status bar for debug builds:

```
  BRANCH  src/main.rs  [3/47]  hunk 2/8        MEM: 12K lines  42 files
```

---

## 8. Scroll Position Calculation

**Current problem:** `scroll_to_current_hunk()` iterates all hunks to calculate the target scroll position. With 100+ hunks per file, this is O(100) per navigation.

### Precomputed hunk offsets

Build a cumulative line count array when a file is selected:

```rust
/// Cumulative logical line offsets for each hunk
pub struct HunkOffsets {
    /// offsets[i] = logical line number where hunk i starts
    pub offsets: Vec<usize>,
    pub total: usize,
}

impl HunkOffsets {
    pub fn build(hunks: &[DiffHunk], ai_state: &AiState, file_path: &str) -> Self {
        let mut offsets = Vec::with_capacity(hunks.len());
        let mut cursor = 0;
        for (i, hunk) in hunks.iter().enumerate() {
            offsets.push(cursor);
            cursor += 1; // hunk header
            cursor += hunk.lines.len();
            cursor += ai_state.comments_for_hunk(file_path, i).len() * 2; // comments
            // + findings in overlay mode, etc.
        }
        Self { offsets, total: cursor }
    }
}
```

Now `scroll_to_current_hunk()` is O(1): `hunk_offsets.offsets[hunk_idx]`.

Rebuild on file selection, mode change, or comment add/delete.

---

## Implementation Priority

### Phase A — Quick wins (do first, biggest impact)

1. **Auto-compaction** (§1) — Pattern-based + size-based. Removes lock files etc. from parse/render entirely. Biggest memory and speed win for typical PRs.
2. **Debounced refresh** (§5) — 5 lines of code, prevents redundant git calls.
3. **Deduplicate git calls** (§5) — Reduce from 3 to max 2 per refresh.

### Phase B — Rendering performance

4. **Virtualized diff rendering** (§2) — Only render visible lines. Eliminates the main scaling bottleneck.
5. **File tree cache + virtualized list** (§4) — Stop rebuilding 500 ListItems per frame.
6. **Precomputed hunk offsets** (§8) — O(1) scroll calculation.

### Phase C — Parse optimization

7. **Two-phase lazy parsing** (§3) — Header-only scan + on-demand file parse. Only matters for very large diffs (500+ files).
8. **Syntax highlighting cache** (§2) — Reduces redundant work across frames.

### Phase D — Polish

9. **Large file warnings** (§6) — UX protection against accidentally loading huge files.
10. **Memory budget tracking** (§7) — Observability for debugging performance issues.
11. **Fast diff hash** (§5) — Use fast hasher for change detection.

## Files Changed

| File | Change |
|------|--------|
| `src/git/diff.rs` | `compact_files()`, `parse_diff_headers()`, `parse_file_hunks()`, two-phase parsing |
| `src/git/status.rs` | `git_diff_raw_file()` for single-file diff |
| `src/ui/diff_view.rs` | Viewport-based rendering, render window calculation |
| `src/ui/file_tree.rs` | `FileTreeCache`, virtualized list rendering |
| `src/app/state.rs` | `DiffFileHeader`, `ParsedFile`, `HunkOffsets`, `MemoryBudget`, debounce logic, compacted file expand |
| `src/ai/review.rs` | Highlight cache in `Highlighter` |
| `src/main.rs` | Debounced refresh, large file gate, expand keybind |
| `src/ui/status_bar.rs` | Memory budget display (debug), compacted file hints |

## Config Additions

```toml
# .er-config.toml

[compaction]
enabled = true
patterns = ["*.lock", "package-lock.json", "yarn.lock", "*.min.js", "*.min.css", "*.snap", "*.pb.go"]
max_lines_before_compact = 500

[performance]
large_file_warning_lines = 2000
virtualize_threshold = 200     # Lines above which viewport rendering kicks in
lazy_parse_threshold = 5000    # Total diff lines above which lazy parsing is used
```
