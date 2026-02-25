# Feature Plans — Split Diff, Heatmap, Blame, Bookmarks, Settings

## 1. Settings System

**Goal:** A config file + in-app settings screen. Every toggleable feature reads from config. The settings overlay lets you toggle features live and writes changes back.

### Config file: `.er-config.toml`

Search order:
1. `{repo_root}/.er-config.toml` (per-repo)
2. `~/.config/er/config.toml` (global)
3. Built-in defaults (everything on)

```toml
# ~/.config/er/config.toml

[features]
split_diff = true         # side-by-side diff in Default/Overlay modes
exit_heatmap = true       # review coverage heatmap on quit
blame_annotations = false # git blame on findings (slower startup)
bookmarks = true          # hunk bookmarks with m/' keys

[agent]
command = "claude"
args = ["--print", "-p", "{prompt}"]

[display]
tab_width = 4
line_numbers = true       # show line numbers in diff
wrap_lines = false        # soft-wrap long lines
```

### Config types (`src/config.rs` — new file)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErConfig {
    #[serde(default)]
    pub features: FeatureFlags,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub display: DisplayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub split_diff: bool,

    #[serde(default = "default_true")]
    pub exit_heatmap: bool,

    #[serde(default)]
    pub blame_annotations: bool,

    #[serde(default = "default_true")]
    pub bookmarks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_cmd")]
    pub command: String,

    #[serde(default = "default_agent_args")]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_tab_width")]
    pub tab_width: u8,

    #[serde(default = "default_true")]
    pub line_numbers: bool,

    #[serde(default)]
    pub wrap_lines: bool,
}

fn default_true() -> bool { true }
fn default_tab_width() -> u8 { 4 }
fn default_agent_cmd() -> String { "claude".into() }
fn default_agent_args() -> Vec<String> {
    vec!["--print".into(), "-p".into(), "{prompt}".into()]
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            split_diff: true,
            exit_heatmap: true,
            blame_annotations: false,
            bookmarks: true,
        }
    }
}

pub fn load_config(repo_root: &str) -> ErConfig {
    let local = format!("{repo_root}/.er-config.toml");
    let global = dirs::config_dir()
        .map(|d| d.join("er/config.toml").to_string_lossy().to_string());

    for path in [Some(local), global].into_iter().flatten() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str::<ErConfig>(&content) {
                return config;
            }
        }
    }

    ErConfig::default()
}

pub fn save_config(config: &ErConfig, repo_root: &str) -> Result<()> {
    // Save to global config (not repo-local, to avoid cluttering repos)
    let dir = dirs::config_dir().unwrap().join("er");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("config.toml");
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}
```

Add `ErConfig` to `App`:

```rust
pub struct App {
    pub config: ErConfig,
    // ... existing fields
}
```

Load in `App::new()`:

```rust
let config = config::load_config(&repo_root);
```

### Settings overlay (`src/ui/settings.rs` — new file)

New `OverlayData` variant:

```rust
pub enum OverlayData {
    WorktreePicker { ... },
    DirectoryBrowser { ... },
    Settings { selected: usize },  // NEW
}
```

Keybinding: `S` (shift-s) in Normal mode opens the settings overlay.

```
┌─── Settings ───────────────────────────────┐
│                                            │
│  Features                                  │
│  ▸ [x] Split diff (side-by-side)           │
│    [x] Exit heatmap                        │
│    [ ] Blame annotations                   │
│    [x] Bookmarks                           │
│                                            │
│  Display                                   │
│    [x] Line numbers                        │
│    [ ] Wrap lines                           │
│    Tab width: 4                             │
│                                            │
│  Agent                                     │
│    Command: claude                          │
│    Args: --print -p {prompt}               │
│                                            │
│         [Save]  [Cancel]                   │
│                                            │
│  j/k nav  ␣ toggle  Enter edit  Esc close  │
└────────────────────────────────────────────┘
```

Navigation: j/k to move cursor, Space to toggle booleans, Enter to edit text/number values (opens inline text input), Esc to close without saving, Enter on [Save] writes to disk.

Changes apply immediately to `app.config` (live preview). [Save] persists to disk. [Cancel] reverts to last saved state.

---

## 2. Split Diff Mode

**Goal:** Side-by-side old/new diff rendering as an alternative to unified diff. Toggled via keybind or settings.

### When it applies

- **Default mode:** Split diff replaces the unified diff view in the right column
- **Overlay mode:** Split diff with AI annotations overlaid
- **SidePanel mode:** NO split — the panel column takes priority
- **AiReview mode:** NO split — it has its own layout

### Layout

```
┌─ Files ──┬─ OLD ──────────────┬─ NEW ──────────────────┐
│          │ src/auth.rs        │ src/auth.rs             │
│ auth.rs  │                    │                         │
│ routes.  │ 43  fn create() {  │ 43  fn create() {       │
│          │ 44    let tok =    │ 44    let tok =          │
│          │ 45    opaque();    │ 45    jwt::encode(&hdr,  │
│          │                    │ 46      &claims, &key)?; │
│          │ 46  }              │ 47    validate(&tok);    │
│          │                    │ 48  }                    │
└──────────┴────────────────────┴─────────────────────────┘
```

### Keybinding

`D` (shift-d) toggles between unified and split diff. Only available in Default and Overlay modes. In SidePanel/AiReview, the keybind does nothing (or shows a notification "Split diff not available in this view").

Also controlled by `config.features.split_diff` — if false, `D` does nothing.

### Implementation

**Files:** `src/ui/diff_view.rs`, `src/app/state.rs`, `src/ui/mod.rs`

Add to `TabState`:

```rust
pub split_diff: bool,  // toggled with D, initialized from config
```

In `src/ui/mod.rs` layout logic — when `split_diff` is true and view mode is Default or Overlay:

```rust
// Instead of one diff column, split into two
let diff_chunks = Layout::horizontal([
    Constraint::Percentage(50),
    Constraint::Percentage(50),
]).split(diff_area);

render_old_side(diff_chunks[0], buf, file, app, hl);
render_new_side(diff_chunks[1], buf, file, app, hl);
```

`render_old_side()` and `render_new_side()`:
- Share the same scroll position (`diff_scroll`)
- Old side shows delete lines and context, skips add lines (replaced with blank)
- New side shows add lines and context, skips delete lines (replaced with blank)
- Both show matching line numbers
- Deleted lines: red bg on old side, blank on new side
- Added lines: blank on old side, green bg on new side
- Context lines: shown on both sides

Hunk headers render as a full-width separator across both columns.

### Scrolling sync

Both columns scroll together (single `diff_scroll` value). Horizontal scroll (`h_scroll`) also syncs. This keeps the old/new sides aligned.

### AI overlay in split mode

In Overlay view mode + split diff:
- Finding banners render below the relevant hunk on the NEW side (right column)
- Risk dot still shows in the file tree (left column)
- Comments render on the NEW side

---

## 3. Review Heatmap on Exit

**Goal:** When quitting `er`, print a compact coverage summary showing which files were actually reviewed, time spent, and review status.

### Tracking data

Add to `TabState`:

```rust
pub struct ReviewMetrics {
    /// Files the user actually visited (selected in file tree)
    pub visited_files: HashSet<String>,

    /// Per-file time tracking (accumulated while file is selected)
    pub time_per_file: HashMap<String, Duration>,

    /// Which hunks were scrolled past (hunk was in viewport)
    pub viewed_hunks: HashMap<String, HashSet<usize>>,

    /// When the session started
    pub session_start: Instant,
}
```

Update metrics on each tick:
- If a file is selected, add elapsed tick duration to `time_per_file[path]`
- If a hunk is in the viewport, add to `viewed_hunks[path]`
- On file selection change, add path to `visited_files`

### Exit output

After terminal restore, if `config.features.exit_heatmap`:

```
── Review Summary ─────────────────────────────── 23m 12s ──

 src/auth/session.rs    ████████░░  4/5 hunks  3m 42s  🔴 high  2 comments  ✓
 src/auth/middleware.rs  ██████████  3/3 hunks  2m 15s  🟡 med
 src/api/routes.rs      ██░░░░░░░░  1/4 hunks  0m 30s  🟢 low
 tests/auth_test.rs     ░░░░░░░░░░  0/2 hunks          🟢 low
 README.md              ░░░░░░░░░░  0/1 hunks          ⚪ info

 3/5 files reviewed  •  8/15 hunks viewed  •  2 comments  •  1/4 checklist ✓
```

Columns:
- File path (truncated if needed)
- Coverage bar: proportion of hunks viewed
- Hunk count: viewed/total
- Time spent (omit if 0)
- Risk level (from `.er-review.json`, or ⚪ if no AI data)
- Comment count (if any)
- ✓ if file is marked reviewed

Color: ANSI escape codes. Coverage bar uses green for viewed, dim for unviewed. Risk dots use the same colors as the TUI.

The summary line at the bottom gives the overall stats.

### Implementation

**Files:** `src/app/state.rs` (metrics tracking), `src/main.rs` (print on exit)

Print function:

```rust
fn print_review_heatmap(app: &App) {
    let tab = app.tab();
    if !app.config.features.exit_heatmap { return; }
    if tab.files.is_empty() { return; }

    let elapsed = tab.metrics.session_start.elapsed();
    eprintln!("\n\x1b[2m── Review Summary ──{:>40}\x1b[0m\n",
        format_duration(elapsed));

    for file in &tab.files {
        let total_hunks = file.hunks.len();
        let viewed = tab.metrics.viewed_hunks
            .get(&file.path)
            .map(|h| h.len())
            .unwrap_or(0);
        let time = tab.metrics.time_per_file
            .get(&file.path)
            .copied()
            .unwrap_or_default();
        let reviewed = tab.reviewed.contains(&file.path);

        let coverage = if total_hunks > 0 {
            (viewed as f32 / total_hunks as f32 * 10.0) as usize
        } else { 0 };

        let bar: String = (0..10).map(|i| {
            if i < coverage { '█' } else { '░' }
        }).collect();

        // ... format and print each line with ANSI colors
    }
}
```

---

## 4. Blame-Aware Findings

**Goal:** When the AI flags a finding on specific lines, show who wrote those lines and when.

### How it works

On `TabState::new()` or when blame is first needed, run:

```
git blame --porcelain <file> -L <start>,<end>
```

Parse the porcelain output to extract:
- Author name
- Author date (relative, e.g. "3 hours ago")
- Commit hash (short)

### Data structure

```rust
#[derive(Debug, Clone)]
pub struct BlameInfo {
    pub author: String,
    pub date_relative: String,
    pub commit_short: String,
}
```

Store in a cache on `TabState`:

```rust
pub blame_cache: HashMap<(String, usize, usize), Vec<BlameInfo>>,
// key: (file_path, line_start, line_end)
```

### When to fetch

**Lazy, not eager** — only fetch blame when:
1. A finding has `line_start`/`line_end` AND
2. The finding is currently visible (in viewport or in side panel)

This avoids slowing startup for large diffs. Cache results so repeat views are instant.

If `config.features.blame_annotations` is false, skip entirely.

### Display

In the diff view, below a finding annotation:

```
 🔴 Token expiry not enforced
    Token is created without exp claim...
    ── blame: Jane Doe • 3 hours ago • a1b2c3d ──
```

In the side panel finding detail:

```
 Token expiry not enforced           HIGH
 ────────────────────────────────────────
 Token is created without exp claim.
 Suggestion: Add .set_expiration(...)

 Written by: Jane Doe (a1b2c3d, 3 hours ago)
```

The blame line is dim/subtle — informational context, not a call-out.

### Implementation

**Files:** `src/git/blame.rs` (new), `src/app/state.rs`, `src/ui/diff_view.rs`, `src/ui/ai_panel.rs`

```rust
// src/git/blame.rs

use std::process::Command;

pub fn blame_lines(
    repo_root: &str,
    file_path: &str,
    start: usize,
    end: usize,
) -> Result<Vec<BlameInfo>> {
    let output = Command::new("git")
        .args([
            "blame", "--porcelain",
            "-L", &format!("{},{}", start, end),
            file_path,
        ])
        .current_dir(repo_root)
        .output()?;

    let text = String::from_utf8_lossy(&output.stdout);
    parse_porcelain_blame(&text)
}

fn parse_porcelain_blame(raw: &str) -> Result<Vec<BlameInfo>> {
    // Porcelain format:
    //   <sha> <orig_line> <final_line> <num_lines>
    //   author <name>
    //   author-time <timestamp>
    //   ...
    // Extract unique (author, commit) pairs
}
```

---

## 5. Diff Bookmarks

**Goal:** Mark hunks for quick return. Lightweight navigation aid for large diffs.

### Data structure

```rust
// In TabState
pub bookmarks: Vec<Bookmark>,

pub struct Bookmark {
    pub file_path: String,
    pub hunk_index: usize,
    pub label: Option<String>,  // optional short note
    pub created_at: Instant,
}
```

### Keybindings

| Key | Action |
|-----|--------|
| `m` | Toggle bookmark on current hunk (add if not bookmarked, remove if already) |
| `'` | Jump to next bookmark (cycles through list) |
| `"` | Jump to previous bookmark |
| `M` | Open bookmark list popup (select to jump) |

If `config.features.bookmarks` is false, these keybinds do nothing.

### Visual indicator

In the diff view, bookmarked hunks show a marker in the gutter:

```
 🔖 @@ -45,8 +45,12 @@ fn create_session
```

Or simpler — a colored diamond/dot on the hunk header line:

```
 ◆ @@ -45,8 +45,12 @@ fn create_session
```

In the file tree, files with bookmarks show a small indicator:

```
  auth.rs ◆       (has bookmarks)
  routes.rs
```

### Bookmark list popup

`M` opens a centered overlay listing all bookmarks:

```
┌─── Bookmarks ──────────────────────────┐
│                                        │
│  1. auth.rs:hunk#2 (L45-52)           │
│  2. middleware.rs:hunk#0 (L12)         │
│  3. auth.rs:hunk#4 (L89-95)           │
│                                        │
│  j/k nav  Enter jump  d delete  Esc   │
└────────────────────────────────────────┘
```

### Persistence

Bookmarks are session-only by default — they don't persist to disk. They're for "I want to come back to this in 5 minutes" not long-term annotation (that's what comments are for).

### Implementation

**Files:** `src/app/state.rs`, `src/main.rs`, `src/ui/diff_view.rs`, `src/ui/overlay.rs`

```rust
impl TabState {
    pub fn toggle_bookmark(&mut self) {
        if !self.config.features.bookmarks { return; }
        let file = match self.files.get(self.selected_file) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let hunk = self.current_hunk;

        // Check if already bookmarked
        if let Some(idx) = self.bookmarks.iter()
            .position(|b| b.file_path == file && b.hunk_index == hunk)
        {
            self.bookmarks.remove(idx);
        } else {
            self.bookmarks.push(Bookmark {
                file_path: file,
                hunk_index: hunk,
                label: None,
                created_at: Instant::now(),
            });
        }
    }

    pub fn jump_to_next_bookmark(&mut self) {
        if self.bookmarks.is_empty() { return; }
        // Find next bookmark after current position
        // Cycle to first if at end
        // Set selected_file and current_hunk to bookmark target
    }

    pub fn jump_to_prev_bookmark(&mut self) {
        // Same but backwards
    }
}
```

---

## Implementation Order

1. **Settings system** — foundation for everything else (config file + `S` overlay)
2. **Bookmarks** — simplest feature, no external deps, good test of settings toggle
3. **Split diff** — significant UI work but self-contained in diff_view.rs
4. **Exit heatmap** — needs metrics tracking wired into the event loop
5. **Blame annotations** — needs git blame parsing, lazy loading, cache

Each feature is gated behind `config.features.*` and can be toggled independently.

---

## New dependencies

```toml
toml = "0.8"    # config file parsing (shared with agent panel)
dirs = "5"      # ~/.config/er/ path (shared with agent panel)
```

## New files

```
src/config.rs           — ErConfig, FeatureFlags, load/save
src/git/blame.rs        — git blame --porcelain parser
src/ui/settings.rs      — settings overlay renderer
```
