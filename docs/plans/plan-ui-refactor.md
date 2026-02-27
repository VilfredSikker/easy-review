# UI Architecture Refactor — Review-First Design

## Problem

The current `er` architecture bundles features in ways that don't scale:

1. **ViewMode = Layout + Data**: `Default | Overlay | SidePanel | AiReview` conflates layout decisions with what data is visible. You can't show GitHub comments in Default mode. You can't see AI findings without switching to a totally different view.

2. **AI gates everything**: `overlay_available()` requires AI review data (or in the agent-terminal-panel variant, questions/comments). Non-AI features are locked behind AI data availability.

3. **Comment types are identical**: Questions (yellow) and GitHub comments (cyan) render identically except for color. No room for type-specific behavior (AI responses, thread depth, sync state).

4. **Navigation modes collide**: `c` means "comment" in Branch mode but "commit" in Staged mode. Arrow keys mean "navigate diff lines" or "navigate comments" depending on hidden `comment_focus` state.

5. **No room for new data**: PR descriptions, CI checks, commit history, code references — each new feature would need its own ViewMode or would get jammed into an existing one.

---

## Design Principles

1. **Review is the primary activity** — The default view should show everything a reviewer needs: the diff, comments, and key metadata.
2. **GitHub is essential, not optional** — PR context (comments, CI, review status) should be visible by default, not gated behind a view mode switch.
3. **AI is supplementary** — AI findings enhance the review but should never be required to access core features.
4. **Layers, not modes** — Information should be toggle-able layers on top of the base diff view, not separate "modes" that replace the entire UI.
5. **One layout, configurable panels** — Instead of 4 different layouts (Default, Overlay, SidePanel, AiReview), have one flexible layout with optional panels.

---

## Current Architecture (What's Wrong)

```
ViewMode::Default    → 2-col (file tree | diff)
ViewMode::Overlay    → 2-col (file tree | diff+inline findings)
ViewMode::SidePanel  → 3-col (file tree | diff | AI panel)
ViewMode::AiReview   → 2-col (risk list | checklist)   ← full-screen AI

Problem: Each mode is a completely different rendering path.
         Comments only appear inline if you're in Overlay or SidePanel.
         Findings only appear if AI data is loaded.
         No way to show GitHub comments + AI findings + PR metadata at the same time.
```

---

## New Architecture: Layers + Panels

### Core Concept

Replace ViewMode (4 discrete modes) with:

1. **One base layout**: file tree (left) + diff view (right) — always present
2. **Toggleable side panel** (right edge) — can show different content
3. **Inline annotations** — independently toggleable layers on the diff

```
┌─────────────────────────────────────────────────────────────┐
│ [repo-name]  Branch  Unstaged  Staged  History   PR #42 ✓  │
├──────────┬──────────────────────────┬───────────────────────┤
│ file     │ diff view                │ context panel         │
│ tree     │                          │ (optional, toggleable)│
│          │ + inline annotations:    │                       │
│          │   · comments (always)    │ Shows one of:         │
│          │   · AI findings (toggle) │  · PR overview        │
│          │   · GitHub threads       │  · AI review summary  │
│          │                          │  · File details       │
│          │                          │  · Code references    │
├──────────┴──────────────────────────┴───────────────────────┤
│ status bar + keybind hints                                  │
└─────────────────────────────────────────────────────────────┘
```

### What Changes

| Old | New |
|-----|-----|
| `ViewMode` enum (4 variants) | `PanelContent` enum + `InlineLayer` flags |
| `overlay_available()` gates view modes | Each layer has its own availability check |
| `v`/`V` cycles through modes | Individual toggle keys per layer |
| Comments only in Overlay+ | Comments always visible inline |
| AI findings only in Overlay+ | AI findings = toggleable inline layer |
| Side panel = AI-only | Side panel = context panel (PR, AI, refs, etc.) |
| AiReview = separate full-screen | AI review summary = panel content option |

---

## 1. Inline Annotations (Layers)

Annotations are independently toggleable overlays on the diff view. Each can be on or off regardless of the others.

### Layer flags

```rust
pub struct InlineLayers {
    /// Personal review questions (yellow) — always on, toggle with `q` visibility
    pub show_questions: bool,           // default: true

    /// GitHub PR comments (cyan) — always on when PR context exists
    pub show_github_comments: bool,     // default: true

    /// AI findings banners (orange) — toggle on/off
    pub show_ai_findings: bool,         // default: false (opt-in)

    /// Risk indicators in file tree
    pub show_risk_indicators: bool,     // default: false (follows ai_findings)
}
```

### Key insight: comments are not "AI"

Comments (questions + GitHub) are core review features. They should always render inline in the diff, regardless of any "AI" toggle. The current system requires Overlay mode to see inline comments, which is wrong.

### Rendering priority (top to bottom within a line range)

1. GitHub review comments (thread-aware, shows reply count)
2. Personal questions (yellow, local-only)
3. AI findings (orange banners, toggle-able)

### Toggle keys

| Key | Layer | Notes |
|-----|-------|-------|
| `a` | AI findings | Toggle inline AI findings + risk indicators |
| (none) | Comments | Always visible. Use `w` to toggle watched files section |

---

## 2. Context Panel (Right Side)

The optional right panel shows contextual information. Only one content type at a time (like browser dev tools tabs).

### Panel content enum

```rust
pub enum PanelContent {
    /// PR overview: title, description, CI checks, review status, labels
    PrOverview,

    /// AI review summary: risk dashboard, findings by severity, checklist
    AiSummary,

    /// File detail: per-file AI summary, all comments grouped, risk
    FileDetail,

    /// Code references: git grep results for selected symbol
    References(ReferencesState),
}
```

### Panel behavior

- **Toggle**: `p` opens/closes the panel
- **Switch content**: When panel is open, `p` cycles through available content
- **Auto-show**: Panel auto-opens to PrOverview when `--pr` flag is used
- **Width**: ~40 columns, or hidden on terminals < 100 cols
- **Independent scroll**: Panel scrolls independently from diff view

### Panel toggle key: `p`

Cycle order: PrOverview → AiSummary → FileDetail → (close) → PrOverview...

Only shows options with available data:
- PrOverview: only when PR context loaded
- AiSummary: only when `.er-review.json` exists
- FileDetail: always available (shows comments even without AI)
- References: only shown when triggered via `g` key

---

## 3. Diff Modes (Unchanged)

The horizontal mode selector stays the same:

```
Branch(1)  Unstaged(2)  Staged(3)  History(4)
```

These control **what diff data is shown**, not how it's displayed. This is the correct separation — data source vs. presentation.

When a PR is loaded, show `PR #42` indicator in the top bar (not a separate mode).

---

## 4. File Tree (Left Panel)

The file tree becomes richer without needing view mode changes:

```
  ~ src/main.rs            +12 −3  💬2  ⚠1
  + src/new_module.rs       +45     💬1
  - src/old.rs              −30
  ~ src/auth.rs             +8  −2  💬3  ⚠2  🔵
  ─────── watched ────────
  👁 .work/agent-state.json     2m ago
```

### Per-file indicators (always visible)

| Icon | Meaning | Source |
|------|---------|--------|
| `💬N` | Total comments (questions + GitHub) | Comment system |
| `⚠N` | AI findings count | AI review (only when `show_ai_findings` on) |
| `🔵` | Has unresolved GitHub review thread | GitHub sync |
| `✓` | Marked as reviewed | Local state |

These indicators don't require any view mode — they're always computed if the data exists. The `⚠N` indicator only appears when the AI findings layer is active.

---

## 5. Status Bar

The status bar consolidates key info without view mode clutter:

### Top bar
```
 er  repo-name  ← Branch  Unstaged  Staged  History →   PR #42 · ✓4 ✗1 · alice:✓
```

- Diff mode selector (1-4 keys, highlighted active)
- PR indicator with CI summary (when PR loaded)
- Review status (when PR loaded)

### Bottom bar
```
 j/k files  n/N hunks  ↑↓ lines  c comment  q question  a AI  p panel  ? help
```

Context-sensitive hints. When panel is open, shows panel-specific keys.

---

## 6. Keybinding Refactor

### Problem: key collisions and hidden modes

Current `c` means "comment" or "commit" depending on DiffMode. Current arrow keys mean "navigate lines" or "navigate comments" depending on `comment_focus`.

### Solution: explicit keys, no hidden state

| Key | Action | Context |
|-----|--------|---------|
| `j`/`k` | Navigate files | File tree |
| `n`/`N` | Next/prev hunk | Diff view |
| `↑`/`↓` | Navigate lines within diff | Diff view |
| `c` | New GitHub comment on current line/hunk | Always (creates draft if no PR) |
| `q` | New question on current line/hunk | Always |
| `C` | New hunk-level GitHub comment | Always |
| `Q` | New hunk-level question | Always |
| `r` | Reply to selected comment | When cursor on a comment |
| `d` | Delete comment | When cursor on a comment |
| `Tab` | Cycle focus: diff → comments → panel | Between focusable areas |
| `a` | Toggle AI findings layer | Global |
| `p` | Toggle/cycle context panel | Global |
| `g` | Code references for symbol at cursor | Line mode |
| `e` | Open in $EDITOR | File/line selected |
| `1`-`4` | Switch diff mode | Global |
| `m` | Mark file as reviewed | File selected |
| `S` | Settings overlay | Global |
| `G` | Pull GitHub comments | PR loaded |
| `P` | Push comments to GitHub | PR loaded, has local comments |
| `Ctrl+q` | Quit | Global |

### Removed keys
- `v`/`V` — No more view mode cycling. Replaced by `a` (AI toggle) and `p` (panel toggle)
- `Tab` for comment focus — Now `Tab` cycles between UI areas cleanly

### Comment navigation

When a comment is visible inline in the diff, the cursor naturally lands on it via `↑`/`↓` line navigation. Comments are "expanded lines" in the diff — they occupy line positions. No separate "comment focus" mode needed.

When the cursor is on a comment line:
- `r` → reply
- `d` → delete
- `Enter` → expand/collapse thread

This eliminates the `comment_focus` boolean and the mode ambiguity.

---

## 7. Data Flow

### Current (bundled)

```
AiState controls:
  → overlay_available() → gates ViewMode cycling
  → view_mode → gates layout (2-col vs 3-col vs full-screen)
  → layout → gates what inline annotations appear
```

### New (layered)

```
InlineLayers (independent booleans):
  → show_questions → always available
  → show_github_comments → available when PR loaded or .er-github-comments.json exists
  → show_ai_findings → available when .er-review.json exists

PanelContent (optional):
  → PrOverview → available when PR loaded
  → AiSummary → available when .er-review.json exists
  → FileDetail → always available
  → References → available on demand (g key)
```

Each data source is independent. Loading AI data doesn't change the layout. Loading PR data doesn't require AI. Comments work regardless of anything.

---

## 8. Migration Path

### Phase 1: Inline comments without view modes

1. Comments (questions + GitHub) render inline in Default mode
2. Remove the `overlay_available()` gate for comment rendering
3. Keep ViewMode for now but make comments independent of it

### Phase 2: Replace ViewMode with layers + panel

1. Add `InlineLayers` struct to `TabState`
2. Add `PanelContent` option to `TabState`
3. `a` key toggles AI findings layer
4. `p` key toggles/cycles context panel
5. Remove `ViewMode` enum entirely
6. Remove `v`/`V` keybinds

### Phase 3: Context panel content

1. PrOverview panel (PR description, CI, reviews)
2. AiSummary panel (risk dashboard, checklist — replaces AiReview full-screen)
3. FileDetail panel (per-file comments + AI summary — replaces SidePanel)
4. References panel (triggered by `g` key)

### Phase 4: Comment navigation refactor

1. Comments become "virtual lines" in the diff viewport
2. `↑`/`↓` moves through both code lines and comment lines
3. Remove `comment_focus` boolean
4. `r`/`d` are context-sensitive (only work when cursor is on a comment)

---

## 9. State Model Changes

### Remove from App/TabState

```rust
// REMOVE
pub view_mode: ViewMode,      // replaced by layers + panel
pub comment_focus: bool,      // replaced by cursor-on-comment detection
```

### Add to TabState

```rust
/// Inline annotation visibility
pub layers: InlineLayers,

/// Optional right panel content (None = panel closed)
pub panel: Option<PanelContent>,

/// Panel scroll position (independent of diff scroll)
pub panel_scroll: u16,
```

### Remove from AiState

```rust
// REMOVE
pub fn overlay_available(&self) -> bool  // no longer needed
pub fn cycle_view_mode() / cycle_view_mode_prev()  // no longer needed
```

---

## 10. Rendering Changes

### `src/ui/mod.rs` — Simplified dispatch

```rust
pub fn draw(f: &mut Frame, app: &App, highlighter: &mut Highlighter) {
    let tab = app.tab();

    // Always: top bar + bottom bar
    render_top_bar(f, app, top_area);
    render_bottom_bar(f, app, bottom_area);

    // Always: file tree (left)
    file_tree::render(f, app, left_area);

    // Always: diff view (center) — with inline layers
    diff_view::render(f, app, highlighter, center_area);

    // Optional: context panel (right)
    if tab.panel.is_some() {
        panel::render(f, app, right_area);
    }

    // Modal overlays on top
    if let Some(overlay) = &app.overlay {
        overlay::render(f, app, overlay);
    }
}
```

Layout math:
- No panel: file_tree (32 cols) + diff (rest)
- With panel: file_tree (32 cols) + diff (rest − 40) + panel (40 cols)
- Narrow terminal (< 100 cols): panel auto-hides

### `src/ui/diff_view.rs` — Layer-aware rendering

```rust
// Per hunk, after rendering code lines:

if tab.layers.show_questions {
    render_questions_for_hunk(/* ... */);
}
if tab.layers.show_github_comments {
    render_github_comments_for_hunk(/* ... */);
}
if tab.layers.show_ai_findings {
    render_findings_for_hunk(/* ... */);
}
```

Each layer renders independently. No `in_overlay` boolean check needed.

---

## Files Changed

| File | Change |
|------|--------|
| `src/app/state.rs` | Remove `ViewMode` usage, add `InlineLayers`, `PanelContent`, panel state. Remove `comment_focus`. |
| `src/ai/review.rs` | Remove `ViewMode` enum, `overlay_available()`, `cycle_view_mode()`. Keep data model. |
| `src/ui/mod.rs` | Replace ViewMode dispatch with panel-aware layout. Single rendering path. |
| `src/ui/diff_view.rs` | Layer-based inline rendering. Comments always visible. AI findings gated by `layers.show_ai_findings`. |
| `src/ui/file_tree.rs` | Always show comment indicators. AI indicators gated by `layers.show_ai_findings`. |
| `src/ui/panel.rs` | **New file.** Context panel rendering (PrOverview, AiSummary, FileDetail, References). |
| `src/ui/status_bar.rs` | Updated top/bottom bar. Remove view mode indicator, add PR status, layer hints. |
| `src/ui/ai_panel.rs` | **Remove.** Replaced by `panel.rs` FileDetail content. |
| `src/ui/ai_review_view.rs` | **Remove.** Replaced by `panel.rs` AiSummary content. |
| `src/main.rs` | Replace `v`/`V` with `a`/`p`. Simplify comment navigation. Remove `comment_focus` toggling. |

---

## What This Enables

1. **PR review without AI**: Open a PR, see description + CI in the panel, comments inline, review files. Zero AI needed.
2. **AI-enhanced review**: Toggle `a` to see findings inline, `p` to see AI summary in panel. AI adds to the review, doesn't replace it.
3. **Mixed workflows**: GitHub comments + AI findings + personal questions all visible at the same time, each independently toggleable.
4. **Future features**: Code references (panel content), commit history (diff mode), blame annotations (inline layer) — all slot in cleanly without new ViewModes.
5. **Simpler mental model**: One layout. Toggle things on/off. No "which mode am I in?" confusion.
