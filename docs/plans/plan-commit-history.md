# Commit History View

## Overview

A new mode alongside Branch, Unstaged, and Staged that shows the commit history of the current branch. The left panel becomes a commit list (instead of a file tree), and the right panel shows the full diff of the selected commit. Navigation mirrors the existing model: j/k to move between commits, n/N to jump between files within a commit, ↑/↓ for lines.

### Why "all files in one view" rather than nested navigation

The current UX for every mode is: left panel picks a top-level item, right panel shows its full diff content that you scroll through. Commit history should follow the same pattern — select a commit on the left, see its complete diff on the right. A nested file sub-menu inside the commit list would break the two-panel model and require a third navigation context that doesn't exist anywhere else in `er`.

Files within a commit are shown as sections in the right panel (file headers → hunks → lines), just like how the current branch view shows hunks. You use n/N to jump between file sections within the commit's diff.

---

## 1. DiffMode Extension

```rust
pub enum DiffMode {
    Branch,
    Unstaged,
    Staged,
    // Recent,   // planned, not yet implemented
    History,
}

impl DiffMode {
    pub fn label(&self) -> &'static str {
        match self {
            // ...existing...
            DiffMode::History => "HISTORY",
        }
    }

    pub fn git_mode(&self) -> &'static str {
        match self {
            // ...existing...
            DiffMode::History => "history", // special handling — not a plain git diff
        }
    }
}
```

Keybind: `5` (or `4` if Recent mode hasn't been implemented yet — check at implementation time).

Status bar: `1 BRANCH  2 UNSTAGED  3 STAGED  5 HISTORY`

---

## 2. Commit List State

History mode needs its own navigation state since the left panel shows commits, not files.

```rust
pub struct HistoryState {
    /// Loaded commits for the current branch
    pub commits: Vec<CommitInfo>,

    /// Currently selected commit index (left panel)
    pub selected_commit: usize,

    /// Parsed diff for the selected commit (right panel)
    pub commit_files: Vec<DiffFile>,

    /// File navigation within the commit diff
    pub selected_file: usize,

    /// Hunk navigation within the selected file
    pub current_hunk: usize,

    /// Line navigation within the selected hunk
    pub current_line: Option<usize>,

    /// Vertical scroll in the diff pane
    pub diff_scroll: u16,

    /// Horizontal scroll
    pub h_scroll: u16,

    /// Total commits available (for lazy loading indicator)
    pub total_count: Option<usize>,

    /// Whether we're loading more commits
    pub loading: bool,
}

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,          // Full SHA
    pub short_hash: String,    // First 7 chars
    pub subject: String,       // First line of commit message
    pub author: String,        // Author name
    pub date: String,          // ISO 8601 timestamp
    pub relative_date: String, // "2 hours ago", "3 days ago"
    pub file_count: usize,     // Number of files changed
    pub adds: usize,           // Total lines added
    pub dels: usize,           // Total lines deleted
}
```

Add to `TabState`:

```rust
pub struct TabState {
    // ...existing fields...

    /// History mode state (only populated when mode == History)
    pub history: Option<HistoryState>,
}
```

---

## 3. Git Commands

### Loading the commit list

```rust
/// Get commit log for the branch (relative to base)
pub fn git_log_branch(base: &str, repo_root: &str, limit: usize) -> Result<Vec<CommitInfo>> {
    let output = Command::new("git")
        .args([
            "log",
            &format!("{}..HEAD", base),
            &format!("--max-count={}", limit),
            "--format=%H%n%h%n%s%n%an%n%aI%n%ar",
            "--shortstat",
        ])
        .current_dir(repo_root)
        .output()?;

    // Parse the output: each commit is 6 lines (format fields) + 1 shortstat line + 1 blank
    parse_git_log(&String::from_utf8_lossy(&output.stdout))
}
```

The `--format` produces:
```
abc1234def5678...       (full hash)
abc1234                 (short hash)
Fix token expiry bug    (subject)
Will                    (author)
2026-02-25T10:00:00+01:00 (date)
2 hours ago             (relative date)

 3 files changed, 45 insertions(+), 12 deletions(-)
```

### Loading a single commit's diff

```rust
/// Get the diff for a single commit
pub fn git_diff_commit(hash: &str, repo_root: &str) -> Result<String> {
    let output = Command::new("git")
        .args([
            "diff",
            &format!("{}^..{}", hash, hash),  // Parent to commit
            "--unified=3",
            "--no-color",
            "--no-ext-diff",
        ])
        .current_dir(repo_root)
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

For merge commits (multiple parents), use `--first-parent` or detect and show a note:

```rust
/// Handle merge commits — diff against first parent
pub fn git_diff_commit_safe(hash: &str, repo_root: &str) -> Result<String> {
    // Try normal diff first
    let output = Command::new("git")
        .args([
            "diff",
            &format!("{}^..{}", hash, hash),
            "--unified=3",
            "--no-color",
            "--no-ext-diff",
        ])
        .current_dir(repo_root)
        .output()?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    // Fallback for initial commit (no parent)
    let output = Command::new("git")
        .args([
            "diff",
            "--root",
            hash,
            "--unified=3",
            "--no-color",
            "--no-ext-diff",
        ])
        .current_dir(repo_root)
        .output()?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

---

## 4. Navigation Model

### Left panel: commit list

| Key | Action |
|-----|--------|
| `j` | Next commit (older) |
| `k` | Previous commit (newer) |

When `selected_commit` changes:
1. Load that commit's diff via `git_diff_commit()`
2. Parse into `commit_files` via existing `parse_diff()`
3. Reset `selected_file`, `current_hunk`, `current_line`, `diff_scroll`

### Right panel: commit diff

| Key | Action |
|-----|--------|
| `n` | Next file within commit |
| `N` | Previous file within commit |
| `↓` | Next line (enters line mode, transitions across hunks and files) |
| `↑` | Previous line |
| `h` / `l` | Horizontal scroll |

This reuses the existing hunk/line navigation logic almost exactly — the only difference is that n/N now move between **files** (which each have their own hunks), rather than between hunks within a single file.

### Why remap n/N to files

In the existing modes, the left panel shows files and j/k navigates them. n/N navigates hunks within the selected file. In History mode, the left panel shows commits and j/k navigates those. The right panel shows the full commit diff with multiple files — so n/N naturally moves between files (the equivalent "sections" within a commit).

Within each file section, ↑/↓ arrow keys navigate lines across hunks, exactly like today. The hunk boundaries are visible but don't need a separate navigation key.

### Alternative: file-level hunk jumping

If a commit touches a file with many hunks, the user might want to jump between hunks within that file. Two options:

**Option A (recommended):** `n/N` moves between files. Within a file, ↑/↓ crosses hunk boundaries naturally (as it does today). No separate hunk key needed — hunks are visually separated and arrows flow through them.

**Option B:** Add `Tab/Shift-Tab` for hunk-jumping within the current file section. More keys, more complexity, but finer control for very large commits.

Start with Option A. Add Option B later if users request it.

---

## 5. Left Panel: Commit List Rendering

When `DiffMode::History` is active, `file_tree.rs` (or a new `commit_list.rs`) renders the commit list instead of the file tree:

```
  ● abc1234  Fix token expiry bug           2h ago
    Will · 3 files  +45 −12
  ─────────────────────────────────
  ○ def5678  Add JWT validation             5h ago
    Will · 1 file   +28 −3
  ─────────────────────────────────
  ○ 789abcd  Refactor auth middleware       1d ago
    Will · 5 files  +112 −89
  ─────────────────────────────────
  ○ bcd0123  Initial auth setup             2d ago
    Will · 8 files  +340 −0
```

Each commit entry takes 2 lines:
- **Line 1:** Selected indicator (`●`/`○`), short hash (dimmed), subject, relative date
- **Line 2:** Author, file count, add/del stats (dimmed)

Separator between commits for visual clarity.

### Styling

- Selected commit: `●` marker + highlighted background (same blue accent as current file selection)
- Current commit's hash: bright text
- Other commits: dimmed
- File count + stats: muted color, right-aligned

---

## 6. Right Panel: Commit Diff Rendering

The right panel shows the selected commit's full diff, rendered as a sequence of file sections:

```
  ── src/auth.rs  ~  +12 −3 ──────────────────
  @@ -45,8 +45,12 @@
    let token = jwt::encode(...);
  - let refresh = old_refresh();
  + let refresh = generate_refresh_token();
  + validate_token_expiry(&token)?;

  ── src/middleware.rs  +  +28 −0 ────────────
  @@ -0,0 +1,28 @@
  + pub fn auth_middleware(...) {
  + ...
```

Each file section starts with a **file header line** showing the path, status symbol, and stats. This is the same rendering used for hunks today, just with file headers as top-level separators.

### Current file indicator

When `n/N` moves between files, the current file header gets a highlight (bright background bar), so the user knows which file section they're in. The status bar also shows the current file name.

### Scroll behavior

When pressing `n` (next file), scroll jumps to the next file header line. Same `scroll_to_current_hunk()` logic, adapted for file offsets within the commit diff.

---

## 7. Lazy Loading

### Initial load

On entering History mode, load the first 50 commits via `git_log_branch()`. Parse the commit list (not the diffs — just metadata). Load the first commit's diff immediately.

### Scroll-triggered loading

When the user navigates past the last loaded commit (pressing `j` at the bottom), load the next batch:

```rust
pub fn load_more_commits(&mut self) -> Result<()> {
    let skip = self.history.as_ref().map_or(0, |h| h.commits.len());
    let new_commits = git_log_branch_skip(&self.base_branch, &self.repo_root, 50, skip)?;

    if let Some(ref mut history) = self.history {
        if new_commits.is_empty() {
            history.total_count = Some(history.commits.len());
        } else {
            history.commits.extend(new_commits);
        }
    }
    Ok(())
}
```

### Diff caching

Keep the last 5 commit diffs in a small LRU cache to avoid re-running `git diff` when the user goes back and forth:

```rust
use std::collections::VecDeque;

pub struct DiffCache {
    entries: VecDeque<(String, Vec<DiffFile>)>,  // (commit_hash, parsed_files)
    max_size: usize,
}

impl DiffCache {
    pub fn new(max_size: usize) -> Self {
        Self { entries: VecDeque::new(), max_size }
    }

    pub fn get(&self, hash: &str) -> Option<&Vec<DiffFile>> {
        self.entries.iter().find(|(h, _)| h == hash).map(|(_, f)| f)
    }

    pub fn insert(&mut self, hash: String, files: Vec<DiffFile>) {
        if self.entries.len() >= self.max_size {
            self.entries.pop_front();
        }
        self.entries.push_back((hash, files));
    }
}
```

---

## 8. Entering and Exiting History Mode

### Entering

```rust
pub fn set_mode(&mut self, mode: DiffMode) {
    if self.mode != mode {
        self.mode = mode;

        if mode == DiffMode::History {
            // Initialize history state if first time
            if self.history.is_none() {
                let commits = git::git_log_branch(&self.base_branch, &self.repo_root, 50)
                    .unwrap_or_default();
                let first_diff = if let Some(c) = commits.first() {
                    let raw = git::git_diff_commit_safe(&c.hash, &self.repo_root)
                        .unwrap_or_default();
                    git::parse_diff(&raw)
                } else {
                    vec![]
                };

                self.history = Some(HistoryState {
                    commits,
                    selected_commit: 0,
                    commit_files: first_diff,
                    selected_file: 0,
                    current_hunk: 0,
                    current_line: None,
                    diff_scroll: 0,
                    h_scroll: 0,
                    total_count: None,
                    loading: false,
                });
            }
        } else {
            // Existing behavior: reset file nav and refresh diff
            self.selected_file = 0;
            self.current_hunk = 0;
            self.current_line = None;
            self.diff_scroll = 0;
            let _ = self.refresh_diff();
        }
    }
}
```

### Preserving state

When switching away from History mode and back, the `HistoryState` is preserved (commit selection, scroll position). Only cleared on full refresh or branch change.

### Exiting

Press `1`, `2`, or `3` to switch back to a normal mode. History state stays in memory for quick re-entry.

---

## 9. Status Bar Adaptations

### Top bar

In History mode, the top bar shows the selected commit info instead of the file path:

```
  abc1234 · Fix token expiry bug · Will · 2 hours ago    3 files  +45 −12
```

### Bottom bar

Mode selector adds History:

```
  1 BRANCH  2 UNSTAGED  3 STAGED  5 HISTORY      src/auth.rs  hunk 2/4
```

The right side shows the current file within the commit diff (if navigating with n/N).

### Keybind hints

Bottom hint line adapts:

```
  j/k: commits  n/N: files  ↑↓: lines  v: view mode  /: search  q: quit
```

---

## 10. Search in History Mode

The `/` search filter should work differently in History mode:

- **Commit search:** Filter commits by subject line or hash
- Not file-based search (since files are secondary in this mode)

```rust
// In visible_commits()
if !self.search_query.is_empty() {
    let q = self.search_query.to_lowercase();
    commits.retain(|c| {
        c.subject.to_lowercase().contains(&q)
            || c.short_hash.contains(&q)
            || c.author.to_lowercase().contains(&q)
    });
}
```

---

## 11. Edge Cases

### Empty history

If the branch has no commits ahead of base (e.g., on main with no changes):

```
  No commits ahead of main.
  Switch to Branch mode (1) to see uncommitted changes.
```

### Merge commits

Show merge commits in the list with a `⊕` indicator. Diff is against the first parent (standard `--first-parent` behavior):

```
  ⊕ abc1234  Merge PR #42: Add auth         1d ago
    Will · 12 files  +340 −89
```

### Very large commits

Apply the same compaction rules from plan-performance.md. If a commit touches 100+ files or has 5,000+ lines, auto-compact lock files and show the large file warning.

### Detached HEAD

If HEAD is detached (not on a branch), History mode shows the log from HEAD with `git log --max-count=50`.

---

## 12. Interaction with Other Features

### AI Review

AI review data (`.er-review.json`) is for the branch diff, not individual commits. In History mode, the AI side panel and overlay are disabled (or show a note: "AI review covers the full branch diff — switch to Branch mode").

### Comments

Comments are tied to the branch diff, not individual commits. In History mode, commenting is disabled. The user should switch to Branch mode to comment.

### File watching

File watcher doesn't trigger refreshes in History mode (commits don't change from file edits). Only a `git fetch` / `git pull` would add new commits, which requires manual refresh (`G` or re-entering the mode).

### Reviewed tracking

The `.er-reviewed` file tracks reviewed files across the branch. In History mode, reviewing individual commits doesn't mark files as reviewed (since a file might appear in multiple commits). Branch-level review tracking remains separate.

---

## Implementation Steps

1. **DiffMode::History + keybind** — Add the enum variant, `5` keybind, status bar label
2. **Git log loading** — `git_log_branch()`, `CommitInfo` struct, log parser
3. **Commit list rendering** — New render function for left panel when in History mode
4. **Commit diff loading** — `git_diff_commit_safe()`, parse into `commit_files`
5. **Navigation wiring** — j/k for commits, n/N for files within commit, ↑/↓ for lines
6. **Right panel rendering** — File-sectioned diff view with file headers as separators
7. **Diff caching** — LRU cache for last 5 commit diffs
8. **Lazy loading** — Load more commits on scroll past end
9. **Search** — Filter commits by subject/hash/author
10. **Edge cases** — Merge commits, empty history, detached HEAD

## Files Changed

| File | Change |
|------|--------|
| `src/app/state.rs` | `DiffMode::History`, `HistoryState`, `CommitInfo`, `DiffCache`, commit navigation methods |
| `src/git/status.rs` | `git_log_branch()`, `git_diff_commit_safe()`, log parser |
| `src/ui/file_tree.rs` | Conditional: render commit list when History mode, file list otherwise |
| `src/ui/diff_view.rs` | File-sectioned rendering for commit diffs (file headers as separators) |
| `src/ui/status_bar.rs` | Add `5 HISTORY` mode label, commit info in top bar, adapted hints |
| `src/main.rs` | `5` keybind, history-aware j/k/n/N routing, lazy load trigger |
