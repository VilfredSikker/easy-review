# Split view is an exact half, and long lines wrap or pan inside their panel

The diff band is always viewport width, so split view is exactly 50/50 no matter how long any line is. Previously the band grew to `max-content`, which let the single widest line in the file dictate the width of every other row and pushed the whole band into horizontal scroll. That defeats split view: the two sides exist to be compared line for line, and they can only be compared if they are the same width. Long lines therefore resolve inside their own panel — word wrap by default, or a per-side horizontal offset driven by that panel's own scrollbar.

## Consequences

- Wrapped row heights are predicted from monospace column arithmetic rather than measured, because row height feeds the virtual window. Anything that changes the wrap width has to invalidate whatever height the window was built from.
- Panning is per side, so left and right sit at different horizontal offsets and must not be kept in sync — the two sides routinely hold different code.
