# Watched Files (Git-Ignored but Visible)

## Problem

Some git-ignored paths (like a `.work/` folder used for agent sync) contain files the user wants to monitor inside `er`, without those files ever being committed. Today `git diff` only operates on tracked files, so ignored paths are completely invisible in the UI.

## Concept

A "watched files" feature that lets users opt specific ignored paths into the `er` file tree. These files appear in a separate section, show content diffs against `/dev/null` (or a cached baseline), and are clearly marked as outside git.

---

## 1. Configuration

### `.er-config.toml`

```toml
[watched]
# Glob patterns for git-ignored paths to include in the UI
paths = [
  ".work/**",
  ".er-reviews/**",
]

# How to diff watched files
# "snapshot" = diff against last-seen snapshot (shows what changed since last open)
# "content"  = always show full file content (like git diff /dev/null)
diff_mode = "snapshot"
```

### Why config, not CLI flag

Watched paths are repo-specific and persistent — the user always wants to see `.work/` in this repo. Config file is the right place. `.er-config.toml` is already planned (see plan-features.md §1).

---

## 2. Loading Watched Files

### Discovery

After loading the normal git diff, scan for watched files separately:

```rust
pub fn discover_watched_files(
    repo_root: &str,
    patterns: &[String],
) -> Result<Vec<WatchedFile>> {
    let mut files = Vec::new();

    for pattern in patterns {
        let full_pattern = format!("{}/{}", repo_root, pattern);
        for entry in glob::glob(&full_pattern)? {
            let path = entry?;
            if path.is_file() {
                let rel_path = path.strip_prefix(repo_root)?.to_string_lossy().to_string();
                let metadata = std::fs::metadata(&path)?;
                let modified = metadata.modified()?;
                let size = metadata.len();

                files.push(WatchedFile {
                    path: rel_path,
                    modified,
                    size,
                });
            }
        }
    }

    // Sort by modification time (most recent first)
    files.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(files)
}
```

### Data structures

```rust
#[derive(Debug, Clone)]
pub struct WatchedFile {
    pub path: String,
    pub modified: SystemTime,
    pub size: u64,
}
```

These are NOT `DiffFile` objects — they have no hunks, no git status. They're a parallel data structure rendered in the same file tree.

### New dep

```toml
glob = "0.3"
```

---

## 3. Diffing Watched Files

Two strategies depending on config:

### Strategy A: Snapshot diff (`diff_mode = "snapshot"`)

Maintain a `.er-snapshots/` directory (gitignored) that stores the last-seen content of each watched file. On open, diff the current file against its snapshot.

```rust
pub fn diff_watched_file(
    repo_root: &str,
    watched: &WatchedFile,
) -> Result<Option<Vec<DiffHunk>>> {
    let current_path = Path::new(repo_root).join(&watched.path);
    let snapshot_path = Path::new(repo_root)
        .join(".er-snapshots")
        .join(&watched.path);

    if !snapshot_path.exists() {
        // First time seeing this file — save snapshot, show full content
        save_snapshot(repo_root, &watched.path)?;
        return Ok(None); // Signal: show as "new file"
    }

    // Use git diff --no-index to diff two arbitrary files
    let output = Command::new("git")
        .args(["diff", "--no-index", "--unified=3", "--no-color"])
        .arg(&snapshot_path)
        .arg(&current_path)
        .current_dir(repo_root)
        .output()?;

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    if raw.is_empty() {
        return Ok(Some(vec![])); // No changes since snapshot
    }

    let parsed = parse_diff(&raw);
    Ok(parsed.into_iter().next().map(|f| f.hunks))
}
```

**Snapshot update:** When the user marks a watched file as "seen" (press `s`), update the snapshot:

```rust
fn save_snapshot(repo_root: &str, rel_path: &str) -> Result<()> {
    let src = Path::new(repo_root).join(rel_path);
    let dst = Path::new(repo_root).join(".er-snapshots").join(rel_path);
    std::fs::create_dir_all(dst.parent().unwrap())?;
    std::fs::copy(src, dst)?;
    Ok(())
}
```

### Strategy B: Content view (`diff_mode = "content"`)

Simply show the full file content with no diff coloring — like a read-only viewer. Simpler, no snapshot management.

```rust
// Use git diff --no-index /dev/null <file> to get a "everything is added" diff
let output = Command::new("git")
    .args(["diff", "--no-index", "--unified=3", "--no-color", "--", "/dev/null"])
    .arg(&current_path)
    .current_dir(repo_root)
    .output()?;
```

### Default recommendation

Start with `"content"` as default — it's simpler and what the user likely wants (just see what's in `.work/`). Snapshot mode can be added later if change-tracking matters.

---

## 4. File Tree Integration

### Separate section

Watched files appear in a visually distinct section below the normal diff files:

```
  ~ src/main.rs            +12 −3
  + src/new_module.rs       +45
  - src/old.rs              −30
  ─────── watched ────────
  👁 .work/agent-state.json     2m ago
  👁 .work/last-prompt.md       15m ago
  👁 .er-reviews/review.json    1h ago
```

The separator line and `👁` icon make it clear these are not part of the git diff.

### Data model in TabState

```rust
pub struct TabState {
    // ... existing fields ...

    /// Git-ignored files opted into visibility
    pub watched_files: Vec<WatchedFile>,

    /// Parsed content for the currently selected watched file
    pub watched_active: Option<WatchedFileContent>,
}

pub struct WatchedFileContent {
    pub path: String,
    /// Hunks if snapshot diff, or full content lines
    pub hunks: Vec<DiffHunk>,
    /// Whether this is a diff or full content view
    pub is_diff: bool,
}
```

### Selection model

The file selection index spans both lists:

```
indices 0..N-1         → normal diff files
index   N              → separator (not selectable, skipped)
indices N+1..N+1+W-1   → watched files
```

When navigating past the last diff file, cursor jumps to the first watched file (and vice versa). The separator is skipped automatically.

### File tree rendering addition

```rust
// After rendering diff files...

if !tab.watched_files.is_empty() {
    // Separator
    items.push(ListItem::new(Line::from(
        Span::styled("─────── watched ────────", Style::default().fg(MUTED))
    )));

    // Watched files
    for watched in &tab.watched_files {
        let age = format_relative_time(watched.modified);
        let icon = if watched_has_changes(&watched) { "👁" } else { "·" };

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!(" {} ", icon), Style::default().fg(WATCHED_COLOR)),
            Span::styled(&watched.path, Style::default().fg(WATCHED_COLOR)),
            Span::raw("  "),
            Span::styled(age, Style::default().fg(MUTED)),
        ])));
    }
}
```

---

## 5. Diff View for Watched Files

When a watched file is selected, the diff view renders differently:

### Content mode

Show file content with line numbers, no +/- prefixes, neutral background:

```
  .work/agent-state.json  (watched · not tracked by git)
  ─────────────────────────────────────────────
  1 │ {
  2 │   "last_run": "2026-02-25T10:00:00Z",
  3 │   "status": "idle",
  4 │   "pending_tasks": 3
  5 │ }
```

### Snapshot diff mode

Standard diff rendering (reuses existing hunk/line rendering), but with a header noting it's against the snapshot:

```
  .work/agent-state.json  (watched · diff vs snapshot from 2h ago)
  @@ -1,5 +1,5 @@
    {
  -   "status": "running",
  +   "status": "idle",
      "pending_tasks": 3
    }
```

### Binary / large file handling

If the watched file is binary or > 10,000 lines, show a summary instead:

```
  .work/model-weights.bin  (watched · binary · 4.2 MB)
```

---

## 6. Refresh Behavior

### On file watcher event

The existing file watcher already monitors the repo root. When a watched file changes:

1. Re-run `discover_watched_files()` to update the list (new/deleted files)
2. If the changed file is currently selected, re-diff it
3. Update the `modified` timestamp in the file tree

### Periodic rescan

Watched files may be created/deleted by external agents. Rescan the glob patterns every 5 seconds (alongside the existing AI file polling):

```rust
// In event loop
if ai_poll_counter % 50 == 0 {  // Every ~5 seconds
    tab.refresh_watched_files()?;
}
```

---

## 7. Gitignore Safety

### Ensure watched files stay ignored

The `.er-snapshots/` directory and any watched paths should never be committed. On first use, verify the paths are gitignored:

```rust
fn verify_gitignored(repo_root: &str, path: &str) -> bool {
    let output = Command::new("git")
        .args(["check-ignore", "-q", path])
        .current_dir(repo_root)
        .output();

    matches!(output, Ok(o) if o.status.success())
}
```

If a watched path is NOT gitignored, show a warning in the UI:

```
  ⚠ .work/ is not in .gitignore — add it to avoid accidental commits
```

### Auto-add to .gitignore (optional)

If the user confirms, append the pattern to `.gitignore`:

```rust
fn suggest_gitignore(repo_root: &str, pattern: &str) -> Result<()> {
    let gitignore = Path::new(repo_root).join(".gitignore");
    let mut file = OpenOptions::new().append(true).create(true).open(gitignore)?;
    writeln!(file, "\n# er watched files")?;
    writeln!(file, "{}", pattern)?;
    Ok(())
}
```

---

## 8. Keybindings

| Key | Context | Action |
|-----|---------|--------|
| `↓`/`↑` | File tree | Navigate across diff files and watched files seamlessly |
| `Enter` | On watched file | Open content/diff view |
| `s` | Viewing watched file (snapshot mode) | Update snapshot ("mark as seen") |
| `w` | Normal | Toggle watched files section visibility |

---

## 9. Interaction with Other Features

### AI Review

Watched files are excluded from AI review — they're not part of the diff being reviewed. The AI panel and findings don't reference them.

### Comments

Users can comment on watched files (useful for noting agent state). Comments are stored in `.er-feedback.json` with the watched file path. These comments are local-only and never pushed to GitHub.

### er-publish

Watched files are excluded from `er-publish` — they're not PR review content.

### Search

The search filter (`/`) applies to both diff files and watched files. Matching works on the full path.

### Compaction

Watched files are not subject to auto-compaction — they're already opt-in, so the user explicitly wants to see them.

---

## Implementation Steps

1. **Config loading** — Parse `[watched]` section from `.er-config.toml` (depends on settings system from plan-features.md)
2. **File discovery** — `discover_watched_files()` with glob matching
3. **File tree section** — Render watched files below separator with `👁` icon and relative timestamps
4. **Selection spanning** — Extend file selection to cover both diff files and watched files
5. **Content viewer** — Render watched file content in diff view area (no diff coloring initially)
6. **Snapshot diffing** — `git diff --no-index` against saved snapshots (optional, phase 2)
7. **Gitignore check** — Warn if watched paths aren't ignored
8. **Refresh** — Periodic rescan of glob patterns for new/deleted files

## Files Changed

| File | Change |
|------|--------|
| `src/git/status.rs` | `discover_watched_files()`, `diff_watched_file()`, `verify_gitignored()` |
| `src/git/diff.rs` | Handle `--no-index` diff output (minor path fixup) |
| `src/app/state.rs` | `WatchedFile`, `WatchedFileContent`, selection spanning, refresh logic |
| `src/ui/file_tree.rs` | Watched section rendering, separator, `👁` icon |
| `src/ui/diff_view.rs` | Content viewer mode for watched files |
| `src/main.rs` | `w` toggle, `s` snapshot update, periodic rescan |

## New Dependencies

```toml
glob = "0.3"
```
