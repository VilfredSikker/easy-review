# ui/ — Rendering

All Ratatui rendering. No state mutation — reads `App` and produces frames.

## Files

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | ~83 | Top-level layout: splits screen, routes to sub-renderers |
| `styles.rs` | ~159 | All colors and `Style` objects. Single source of truth. |
| `highlight.rs` | ~79 | Syntect-based syntax highlighting |
| `diff_view.rs` | ~362 | Right panel: hunks, line numbers, syntax-highlighted diff |
| `file_tree.rs` | ~330 | Left panel: file list with status/risk indicators + watched files section |
| `status_bar.rs` | ~456 | Top bar (branch/tabs/modes) + bottom bar (key hints/input) |
| `overlay.rs` | ~217 | Modal popups: worktree picker, directory browser |
| `ai_panel.rs` | ~286 | AI side panel (SidePanel view): findings + comments per file |
| `ai_review_view.rs` | ~376 | Full-screen AI review: file risks + checklist |

## Layout (mod.rs)

Screen splits into three vertical zones:

```
┌─────────────────────────────────────────┐
│ top bar (2-3 rows: tabs + branch + modes)│
├──────────────┬──────────────────────────┤
│ file tree    │ diff view                │
│ (32 cols)    │ (rest)    [+ AI panel]   │
├──────────────┴──────────────────────────┤
│ bottom bar (1+ rows: hints or input)    │
└─────────────────────────────────────────┘
```

Layout varies by `ViewMode`:
- `Default` / `Overlay` — 2 columns (file tree + diff)
- `SidePanel` — 3 columns (file tree + diff + AI panel, min 40 + min 30)
- `AiReview` — full screen (replaces file tree and diff entirely)

## styles.rs — Color System

Cool blue-undertone dark theme. All colors are constants, all styles are functions.

Background layers: `BG` (darkest) → `SURFACE` → `PANEL` → `BORDER`
Diff colors: `ADD_BG/ADD_TEXT` (green), `DEL_BG/DEL_TEXT` (red), `HUNK_BG`
AI colors: `STALE`, `FINDING_BG`, `COMMENT_BG`, `LINE_CURSOR_BG`
Watched colors: `WATCHED_TEXT`, `WATCHED_MUTED`, `WATCHED_BG`

Rule: never use raw `Color::*` outside this file.

## highlight.rs — Syntax Highlighting

`Highlighter` is created once in main.rs, passed to `ui::draw()`, used by `diff_view`.

`highlight_line(line, filename, base_style)` — detects language from extension, applies `base16-ocean.dark` theme. The base style carries diff background color; only foreground is overridden by syntax highlighting. This preserves green/red diff backgrounds while adding syntax colors.

## diff_view.rs — Diff Rendering

Builds a flat `Vec<Line>` from all hunks eagerly, then wraps in `Paragraph::scroll()`. Each hunk gets:
- Header line with `▶` marker on current hunk
- Diff lines with gutter (old_num/new_num + `│`) and syntax-highlighted content
- Optional AI finding banners (Overlay mode only) and comment banners after each hunk

Stale AI data renders in dimmed yellow with `[stale]` tag.

Note: `SidePanel` mode shows risk in file header but does NOT insert inline finding banners (those go in the side panel).

## status_bar.rs — Dynamic Height Bars

Both bars compute their height before layout because they word-wrap to fit terminal width.

Top bar: tab row (if multi-tab) + branch info + mode indicators (1 BRANCH / 2 UNSTAGED / 3 STAGED).
Bottom bar: in Normal mode shows packed key hints; in Search/Comment mode shows input prompt.

`build_hints(app)` is context-sensitive — changes based on `InputMode`, `ViewMode`, multi-tab state, AI data availability.

## Important Patterns

- `f.render_widget(Clear, popup)` in overlay.rs — clears background before rendering popup
- Scroll is shared: `tab.diff_scroll` controls both diff_view and ai_panel in SidePanel mode
- `shorten_path()` in file_tree.rs truncates directories with `…/filename` to fit column width
- Finding banners truncated to `area.width - 6` with ellipsis
- `render_watched()` in diff_view.rs handles content and snapshot diff display for watched files (no syntax highlighting — uses plain styled spans to avoid lifetime issues)
- file_tree.rs renders watched files below a `── watched ──` separator with ◉ icon (⚠ if not gitignored), relative timestamps
