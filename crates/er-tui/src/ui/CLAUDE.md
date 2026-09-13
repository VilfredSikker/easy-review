# ui/ — rendering surface

Read-only over `App`: renderers take `&App` and produce frames. Nothing here
mutates state, runs an event loop, or is async. State a renderer needs must
already exist on `App`/`TabState` — derive it in the engine rather than
computing it mid-frame (`docs/adr/0006-engine-state-is-the-ui-contract.md`).

Overlays render last and `Clear` their area first. A popup drawn before the
panels, or without `Clear`, shows the frame underneath through it.

## Rendering invariants

**Viewport-based rendering.** Only visible rows are built. The diff view
virtualizes above `VIRTUALIZE_THRESHOLD` (200 total diff lines), building lines
from `scroll - 20` to `scroll + height + 20` and exiting early past that; the
file tree slices the visible window before constructing `ListItem`s. Building
every `Line` for the file and letting the widget clip it is a performance
regression nothing catches — a 20-row viewport goes from ~60 built lines to
5,000.

**The diff view pads its slice.** Visible rows are pre-sliced and padded with
bg-styled empty `Line`s to the full inner height. Ratatui's double buffer reuses
the previous frame's cells for rows below a `Paragraph`'s last line, so a short
line vector bleeds stale content. Dropping the padding, or going back to
`Paragraph::scroll()` for vertical scroll, brings that back.

**Highlight cache.** `er_engine::highlight::Highlighter` keys its LRU cache by
line content + filename + syntect theme name, 10,000 entries, evicting a
quarter. Content, not position — that is what makes scrolling and frame-to-frame
re-render hit; a key by line index turns it into a miss. The theme name is part
of the key, so switching themes refills it. Desktop highlights in Shiki instead
(`docs/adr/0014-client-side-highlighting.md`).

The TUI adapter converts the engine's `#RRGGBB` spans to ratatui `Color` and
layers each on a base style that already carries the diff row background, so
only the foreground is overridden. A highlight span that sets its own style
drops the add/del row tint.

## Color

Renderers resolve tokens through `styles.rs` accessors; raw `Color::*` values
exist only in the token tables in `themes.rs` and in the hex conversion in
`highlight.rs`. A color written anywhere else breaks every theme except the one
it was picked in. The active theme is process-global, refreshed from config at
the top of `draw()` so a settings change restyles live, and graphite until the
first frame (`docs/adr/0030-shared-theme-tokens.md`).
