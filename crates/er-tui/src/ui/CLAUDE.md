# ui/ — Rendering

All Ratatui rendering for the terminal `er`. No state mutation — reads `App`
and produces frames. `draw()` applies the configured theme each frame, then
splits the screen and routes to sub-renderers.

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | Top-level layout: bar heights, screen split, routes to sub-renderers |
| `styles.rs` | All colors and `Style` helpers. Single source of truth. |
| `themes.rs` | Theme registry (semantic tokens + per-theme syntect theme) |
| `highlight.rs` | TUI adapter over `er_engine::highlight::Highlighter` with content-hash LRU cache |
| `diff_view.rs` | Right panel: viewport-based diff rendering, inline comments/findings, sticky file header, compacted files, History mode multi-file diff |
| `file_tree.rs` | Left panel: file list with status/risk indicators, watched files section (commit list in History mode) |
| `panel.rs` | Side panel content renderers: FileDetail, AiSummary, PrOverview |
| `status_bar.rs` | Top bar (tabs/branch/modes) + bottom bar (hints or input), AI status badges |
| `overlay.rs` | Modal popups: worktree picker, directory browser, filter history |
| `settings.rs` | Settings overlay (live config editing) |
| `utils.rs` | Shared utilities (`word_wrap`) |

## Layout (mod.rs)

Screen splits into three vertical zones:

```
┌─────────────────────────────────────────┐
│ top bar (2-3 rows: tabs + branch + modes)│
├──────────────┬──────────────────────────┤
│ file tree    │ diff view                │
│ (32 cols)    │ (rest)    [+ side panel] │
├──────────────┴──────────────────────────┤
│ bottom bar (1+ rows: hints or input)    │
└─────────────────────────────────────────┘
```

The side panel appears when `tab.panel` is `Some(PanelContent)` (cycled with
the panel toggle keys); inline annotation visibility is controlled by
`InlineLayers`, not a view mode.

## styles.rs / themes.rs — Color System

Semantic tokens resolved through the active theme: background layers
(`bg` → `surface` → `panel` → `border`), diff colors (`add`/`del`/`hunk`),
accents, and interactive states. Rule: never use raw `Color::*` outside
`styles.rs`/`themes.rs`.

## highlight.rs — Syntax Highlighting

Thin adapter over the engine's syntect-based `Highlighter`. Theme comes from
`themes.rs` (`syntect_theme` per theme). Results are cached by content+filename
hash with LRU eviction; the base style carries the diff background color while
syntax highlighting overrides only the foreground.

## diff_view.rs — Diff Rendering

Viewport-based: only builds `Line` objects for visible rows (+ buffer) above
the virtualization threshold. Each hunk gets a header line with a `▶` marker on
the current hunk, gutter (old/new line numbers + `│`), and syntax-highlighted
content. Inline comment and finding banners render after their target line.
Stale AI data renders dimmed with a `[stale]` tag. Compacted files render a
summary row expandable with `Enter`. A sticky file path header pins the current
file at the top of the viewport.

## status_bar.rs — Dynamic Height Bars

Both bars compute their height before layout because they word-wrap to fit
terminal width. Top bar: tab row (if multi-tab) + branch info + mode
indicators. Bottom bar: packed key hints in Normal mode, input prompt in
Search/Comment/Filter modes. `build_hints(app)` is context-sensitive.

## Important Patterns

- `f.render_widget(Clear, popup)` in overlay.rs — clears background before rendering popup
- `shorten_path()` in file_tree.rs truncates directories with `…/filename` to fit column width
- Finding banners truncated to `area.width - 6` with ellipsis
- `render_watched()` in diff_view.rs handles content and snapshot diff display for watched files (plain styled spans, no syntax highlighting)
- file_tree.rs renders watched files below a `── watched ──` separator with ◉ icon (⚠ if not gitignored), relative timestamps
