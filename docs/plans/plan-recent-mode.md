# Recent Mode — 4th Diff View

## Overview

A new `DiffMode::Recent` (key `4`) that shows the same branch diff but sorts files by filesystem modification time (newest first). Surfaces whatever you're actively working on at the top of the file tree.

```
 1 BRANCH  2 UNSTAGED  3 STAGED  4 RECENT
```

## How it differs from Branch mode

| | Branch (1) | Recent (4) |
|---|---|---|
| **Diff source** | `git diff <base>` | `git diff <base>` (identical) |
| **File sort** | Alphabetical (or AI order) | By mtime, newest first |
| **AI data** | Same diff_hash | Same diff_hash (shared) |
| **Use case** | Systematic review | "What was I just working on?" |

Same diff, same hunks, same findings — just a different file order. This means AI data, reviewed state, comments, and bookmarks all carry across when switching between Branch and Recent.

## Implementation

### Step 1: Add DiffMode::Recent

**File:** `src/app/state.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiffMode {
    Branch,
    Unstaged,
    Staged,
    Recent,
}

impl DiffMode {
    pub fn label(&self) -> &'static str {
        match self {
            DiffMode::Branch => "BRANCH DIFF",
            DiffMode::Unstaged => "UNSTAGED",
            DiffMode::Staged => "STAGED",
            DiffMode::Recent => "RECENT",
        }
    }

    pub fn git_mode(&self) -> &'static str {
        match self {
            // Recent uses the same diff as Branch
            DiffMode::Branch | DiffMode::Recent => "branch",
            DiffMode::Unstaged => "unstaged",
            DiffMode::Staged => "staged",
        }
    }
}
```

Since `git_mode()` returns `"branch"` for both Branch and Recent, `refresh_diff()` produces the same raw diff and same `diff_hash`. The only difference is the post-processing sort.

### Step 2: Sort files by mtime after parsing

**File:** `src/app/state.rs` — in `refresh_diff_impl()`

After `self.files = git::parse_diff(&raw)`, add the mtime sort:

```rust
fn refresh_diff_impl(&mut self, recompute_branch_hash: bool) -> Result<()> {
    let raw = git::git_diff_raw(self.mode.git_mode(), &self.base_branch, &self.repo_root)?;
    self.files = git::parse_diff(&raw);

    // Sort by mtime in Recent mode
    if self.mode == DiffMode::Recent {
        self.sort_files_by_mtime();
    }

    // ... rest of existing logic (diff_hash, AI state, etc.)
}
```

The sort function:

```rust
fn sort_files_by_mtime(&mut self) {
    use std::fs;
    use std::time::SystemTime;

    let repo_root = self.repo_root.clone();

    self.files.sort_by(|a, b| {
        let mtime_a = fs::metadata(format!("{}/{}", repo_root, a.path))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let mtime_b = fs::metadata(format!("{}/{}", repo_root, b.path))
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        // Newest first (reverse chronological)
        mtime_b.cmp(&mtime_a)
    });
}
```

For deleted files (which don't exist on disk), `metadata()` fails and they sort to the bottom with `UNIX_EPOCH`. This is reasonable — you can't be "actively working on" a deleted file.

### Step 3: Keybinding

**File:** `src/main.rs`

```rust
KeyCode::Char('4') => {
    app.tab_mut().set_mode(DiffMode::Recent);
}
```

### Step 4: Status bar

**File:** `src/ui/status_bar.rs`

Add the 4th mode to the top bar:

```rust
let mut modes: Vec<Span> = vec![
    Span::raw(" "),
    Span::styled(" 1 ", mode_style(DiffMode::Branch, tab.mode)),
    Span::styled(" BRANCH ", mode_style(DiffMode::Branch, tab.mode)),
    Span::raw(" "),
    Span::styled(" 2 ", mode_style(DiffMode::Unstaged, tab.mode)),
    Span::styled(" UNSTAGED ", mode_style(DiffMode::Unstaged, tab.mode)),
    Span::raw(" "),
    Span::styled(" 3 ", mode_style(DiffMode::Staged, tab.mode)),
    Span::styled(" STAGED ", mode_style(DiffMode::Staged, tab.mode)),
    Span::raw(" "),
    Span::styled(" 4 ", mode_style(DiffMode::Recent, tab.mode)),
    Span::styled(" RECENT ", mode_style(DiffMode::Recent, tab.mode)),
];
```

### Step 5: File tree — show relative time

**File:** `src/ui/file_tree.rs`

In Recent mode, show a relative timestamp next to each file instead of (or in addition to) the +/- line counts:

```
 auth.rs          2m ago   +34 -12
 middleware.rs    15m ago   +8  -3
 routes.rs        1h ago   +22 -0
 tests/auth.rs    3h ago   +45 -10
```

```rust
fn format_relative_time(mtime: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or_default();

    let secs = elapsed.as_secs();
    if secs < 60 { return format!("{}s ago", secs); }
    if secs < 3600 { return format!("{}m ago", secs / 60); }
    if secs < 86400 { return format!("{}h ago", secs / 3600); }
    format!("{}d ago", secs / 86400)
}
```

Only show the timestamp column in Recent mode. In Branch/Unstaged/Staged, the file tree renders as before.

### Step 6: Auto-refresh sort on file watch

The file watcher already detects file changes and calls `refresh_diff_quick()`. Since that calls `refresh_diff_impl()` which applies the mtime sort, the file order updates automatically when you save a file externally.

No additional work needed — it falls out of the existing architecture.

### Step 7: Bottom bar hint

In the bottom hint bar, show `4` as a keybind option:

```rust
Hint::new("1-4", " mode "),
```

(Currently shows `1-3`.)

## Edge cases

**Switching between Branch and Recent preserves position by filename:**
When switching from Branch to Recent (or back), the selected file should stay the same even though its index changes. In `set_mode()`:

```rust
pub fn set_mode(&mut self, mode: DiffMode) {
    if self.mode != mode {
        // Remember current file path
        let current_path = self.files
            .get(self.selected_file)
            .map(|f| f.path.clone());

        self.mode = mode;
        let _ = self.refresh_diff();

        // Restore selection by path
        if let Some(path) = current_path {
            if let Some(idx) = self.files.iter().position(|f| f.path == path) {
                self.selected_file = idx;
            } else {
                self.selected_file = 0;
            }
        } else {
            self.selected_file = 0;
        }

        self.current_hunk = 0;
        self.current_line = None;
        self.diff_scroll = 0;
    }
}
```

**AI order toggle (`O` key) in Recent mode:**
The `O` key toggles between AI-suggested order and alphabetical in Branch mode. In Recent mode, `O` could toggle between mtime order and alphabetical. Or it could be disabled. Simplest: `O` has no effect in Recent mode (mtime is the whole point).

**er-review scope support:**
The `/er-review` skill now supports scopes (`branch`, `unstaged`, `staged`). Recent doesn't need its own scope — it uses `branch` scope since the diff is identical. The skill doesn't need to know about Recent mode.

## Files changed

| File | Change |
|---|---|
| `src/app/state.rs` | Add `DiffMode::Recent`, `sort_files_by_mtime()`, preserve file selection in `set_mode()` |
| `src/main.rs` | Add `KeyCode::Char('4')` handler |
| `src/ui/status_bar.rs` | Add 4th mode to top bar, update hint from `1-3` to `1-4` |
| `src/ui/file_tree.rs` | Show relative time column in Recent mode |
| `src/git/status.rs` | No changes (git_diff_raw already handles "branch" mode) |
