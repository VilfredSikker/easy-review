# Themes are semantic tokens shared by both front ends

One token set per theme drives the TUI and the desktop app: `crates/er-tui/src/ui/themes.rs` defines the palettes and `desktop-ui/src/lib/themes.ts` mirrors them, each front end resolving the same names — through `ui/styles.rs` in Rust, through CSS custom properties on the document root in the webview. A theme supplies only anchors (canvas, surface, panel, border, text ladder, accents); borders, diff backgrounds, and interactive states are alpha-composited over the canvas. So a new theme is a data edit that touches no rendering code, and the two front ends cannot drift into looking like different products.

## Consequences

- Retired names (`ocean-depth`, `moonlight`, `daybreak`, `tokyo-night*`) still resolve, aliased to their nearest current theme, so a `display.theme` saved in config never breaks a launch. New aliases go in both files.
- Nothing compares the Rust and TypeScript palettes. A theme added to one side alone renders as the fallback (graphite) on the other, silently.
- The derived diff backgrounds reach past looks: the desktop's contrast pass (`desktop-ui/src/lib/diffContrast.ts`) corrects Shiki token colors against them to clear WCAG AA on every theme. Too little separation between a token and a composited add/delete background surfaces as the correction shifting that color, not as a failing check.
