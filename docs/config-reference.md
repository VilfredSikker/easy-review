# Configuration Reference

`er` loads configuration from TOML files. Settings are deep-merged: global provides shared defaults, per-repo overrides individual fields.

## Config Files

| Location | Purpose |
|----------|---------|
| `~/.config/er/config.toml` | Global defaults (shared across all repos) |
| `.er-config.toml` (repo root) | Per-repo overrides |

**Priority:** per-repo > global > built-in defaults.

Per-repo only needs to specify fields that differ from your global config. Unspecified fields inherit from global, then from built-in defaults.

## Live Editing

Press `S` inside `er` to open the settings overlay. Changes apply immediately. Press `s` to persist to `~/.config/er/config.toml`, or `Esc` to revert.

## All Options

### `[features]`

Feature toggles. All default to `true` except `blame_annotations`.

```toml
[features]
split_diff = true          # Side-by-side diff view
exit_heatmap = true        # Show review heatmap on exit
blame_annotations = false  # Show git blame inline (off by default)
bookmarks = true           # Enable diff bookmarks
view_branch = true         # Enable branch diff mode (key: 1)
view_unstaged = true       # Enable unstaged diff mode (key: 2)
view_staged = true         # Enable staged diff mode (key: 3)
ai_overlays = true         # Enable AI overlay views (key: v/V)
```

### `[display]`

Rendering options.

```toml
[display]
tab_width = 4        # Spaces per tab character (1-16)
line_numbers = true  # Show line numbers in diff view
wrap_lines = false   # Wrap long lines instead of horizontal scroll
```

### `[agent]`

AI agent command configuration. Used when triggering AI review from within `er`.

```toml
[agent]
command = "claude"                      # Binary to invoke
args = ["--print", "-p", "{prompt}"]    # Arguments ({prompt} is replaced)
```

### `[watched]`

Monitor git-ignored files (e.g., agent work directories, sidecar files). Watched files appear in the file tree alongside tracked changes.

```toml
[watched]
paths = [".work/**/*", ".er/**/*"]  # Glob patterns for files to watch
diff_mode = "content"               # "content" (show file) or "snapshot" (diff against saved baseline)
```

**Note:** Watched files should be in `.gitignore`. `er` warns if they aren't.

## Example Configs

### Global (`~/.config/er/config.toml`)

Shared preferences across all repos:

```toml
[features]
blame_annotations = true

[display]
tab_width = 2
wrap_lines = true

[watched]
paths = [".work/**/*", ".er/**/*"]
```

### Per-repo (`.er-config.toml`)

Only override what's different for this repo:

```toml
[display]
tab_width = 4

[watched]
paths = [".work/**/*", ".er/**/*", "logs/**/*.log"]
diff_mode = "snapshot"
```

Result: `blame_annotations = true` and `wrap_lines = true` from global, `tab_width = 4` and extra watched paths from local.
