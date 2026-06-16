# Easy Review — Themes & App Icon Handoff

Two brand-system updates ship together: a **rebuilt theme system** (token-driven, contrast-checked) and a **new app icon** (Review List) that replaces the old `er` text mark everywhere.

- **Theme gallery / source of truth:** `Theme System.html` + `theme-tokens.js`
- **Icon source + exports:** `brand/app-icon/` (SVG + PNG, Dark & Light)
- **In-product reference:** `Easy Review Mobile.html` (mark + dark themes wired into Tweaks)

---

## Part 1 — Themes

### What changed & why
The old set had two problems the user flagged: the white theme read **blue-ish**, and several themes were **too low-contrast**. It also carried **four near-identical Tokyo Night** variants. The rebuild:

- Collapses the four Tokyo Nights into **one** well-tuned dark theme (**Midnight**).
- Replaces the blue-ish white with **Paper** — a genuinely *warm* white, no blue cast.
- Adds true **AAA accessibility** themes (Contrast Dark / Light) in place of the weak "High Contrast".
- Makes every value a **role-based token**, so a theme is a pure data swap — no hard-coded hex in components.

### The 8 themes
| Group | Theme | Notes |
|---|---|---|
| Dark | **Graphite** | Neutral near-black. **Default.** |
| Dark | **Slate** | Cool blue-grey, calm, high legibility. |
| Dark | **Midnight** | Deep indigo. Keeps the loved Tokyo palette for semantics. |
| Dark | **Ember** | Warm charcoal for night reading. |
| Light | **Paper** | Warm white (`#faf8f4`), warm-grey ink. Fixes the blue cast. |
| Light | **Daylight** | Crisp cool light, kept high-contrast. |
| A11y | **Contrast Dark** | Pure black, AAA text, vivid semantics. |
| A11y | **Contrast Light** | Pure white, near-black ink, AAA. |

### Token roles (every theme defines all of these)
| Token | Role |
|---|---|
| `bg` / `bg1` / `bg2` / `bg3` | canvas · raised card · nested panel/input · chip/hover |
| `line` / `line2` | hairline border · stronger border |
| `tx` / `tx2` / `tx3` | text primary · secondary · tertiary (`tx2` = default icon colour) |
| `icon` | default icon colour |
| `accent` / `onAccent` / `accentSoft` | brand (er) · glyph on accent · accent tint |
| `red` / `amber` / `blue` | severity HIGH · MED · LOW |
| `cyan` / `purple` / `green` | comments&findings · PR#&merged · approved&synced&additions |
| `add` / `del` | diff add / delete rails (line bg derived at ~15%) |
| `sxKey` `sxFn` `sxStr` `sxNum` `sxCom` `sxPun` | syntax colours |

### Contrast targets
`tx` ≥ 7:1 on `bg` (AAA body) · `tx2` ≥ 4.5:1 (AA) · `tx3` ≥ 3:1 (AA large/UI). Semantic colours are **darkened on light themes** so labels hold ≥ ~4:1.

### How to apply
`theme-tokens.js` exports `window.ER_THEMES` (array of `{id, name, group, mode, tokens}`) and `applyTheme(el, theme)`, which sets one CSS custom property per role on `el` and stamps `data-mode`. Components read only the variables — adding or editing a theme touches **data alone**.

```js
applyTheme(document.documentElement, ER_THEMES.find(t => t.id === 'graphite'));
```

> **In the mobile reference**, all eight themes are wired into the Tweaks panel (`Theme` → Palette) and switch the whole app live. Hard-coded on-fill text colours were tokenised — `--on-accent` (text on an accent fill) and `--on-pos` (text on a green fill) — so labels invert correctly on light tiles, and the device status-bar text flips dark on light themes via the frame's `dark` prop (driven by each theme's `mode`).

---

## Part 2 — App icon

### The mark — "Review List"
A rebuild of the existing favicon: **severity badges leading tinted finding rows.** It reads like the Easy Review findings panel — the most product-true option, chosen over the diff-bar marks. Badge colours map to the severity palette: **blue = low · green = pass · red = high.**

### Two tiles only (Sunset retired)
| Variant | Tile gradient | Use |
|---|---|---|
| **Dark** | `#1d2331 → #0c0e14` | dark UI, App Store, default |
| **Light** | `#ffffff → #eceef1` | light contexts (severity shades darkened to hold on white) |

Geometry (64-grid): squircle `rx 14.3`; three rows at y = 18 / 32 / 46; badge `9.2×9.2 rx 2.9` at x=15; row bar `22×6.4 rx 3.2` at x=27.5, tinted (~0.34 dark / 0.26 light). Verified legible down to **16px**.

### Files (`brand/app-icon/`)
- **SVG source:** `review-list-dark.svg`, `review-list-light.svg`
- **PNG** (`png/`): Dark + Light at **1024 · 512 · 256 · 180 · 120 · 64 · 32 · 16** — transparent rounded corners.
- Covers App Store (1024), iOS (180/120), Android·PWA (512/256), favicon (32/16).

### Where it now appears (the `er` text mark is gone)
| Surface | Before | After |
|---|---|---|
| Mobile top bar (`AppBar`) | `er` text tile | Review List mark (`AppMark`, 30px) |
| Global drawer header | `er` text tile | `AppMark` 32px |
| Home screen top bar | `er` text tile | `AppMark` 30px |
| Mobile intro / design-system header | `er` avatar | `AppMark` 38px |
| Theme System header + preview top bar | `er` text | `RLMark` |
| Page favicons (both HTML) | — | `review-list-dark-32.png` |

`AppMark` is defined in `er-ui.jsx` and exported to `window`; `ErLogo` now simply renders it, so every existing call site updates automatically. The Theme System uses an equivalent `RLMark` in `theme-preview.jsx`.

### Implementation note
The icon tile is a **fixed dark mark** and intentionally does **not** recolour with the theme (an app icon is a constant). It sits cleanly on both dark and light surfaces. If you ever want a theme-reactive in-header monogram instead, that's a separate, simpler mark — keep the app/store icon fixed.

---

## File map
| File | Contents |
|---|---|
| `Theme System.html` | Interactive 8-theme gallery + token reference |
| `theme-tokens.js` | **Theme source of truth** + `applyTheme()` |
| `theme-preview.jsx` | Themed preview cluster · `RLMark` |
| `brand/app-icon/*.svg` | Vector icon source (Dark, Light) |
| `brand/app-icon/png/*` | PNG export set (Dark, Light × 8 sizes) |
| `Easy Review Mobile.html` | Mobile reference — `AppMark` + dark themes in Tweaks |
| `er-ui.jsx` | `AppMark` component (exported) |
| `App Icon — Review List.html` | Final icon spec sheet |
