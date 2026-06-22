/**
 * Desktop port of the shared theme system (mirrors
 * `crates/er-tui/src/ui/themes.rs` and the design's `theme-tokens.js`).
 *
 * Each theme is a pure data swap of role tokens; every value the desktop UI
 * needs that isn't a raw token (borders, diff backgrounds, selection) is
 * *derived* from the tokens by alpha-compositing over the canvas, so adding or
 * editing a theme touches data alone.
 *
 * Themes override the CSS custom properties declared in `app.css` (`@theme`
 * block) on the document root, so every Tailwind utility and semantic alias
 * (`--color-bg`, `--color-card`, …) follows the active theme. The
 * `--color-ink-*` ladder is interpolated from the theme's four anchor surfaces
 * (bg → surface → panel → border) so intermediate shades stay consistent for
 * light and dark palettes alike.
 */

import { DEFAULT_SYNTAX_THEME_ID } from "./syntaxThemes";

export interface AppTheme {
  name: string;
  /** True for light palettes — drives `color-scheme` on the root element. */
  light: boolean;
  /** Shiki theme id from `syntaxThemes.ts` used for diff syntax highlighting. */
  syntaxThemeId: string;

  // Background layer
  bg: string;
  surface: string;
  panel: string;
  border: string;

  // Text layer (textBright is always the strongest body color)
  text: string;
  textBright: string;
  textDim: string;
  textMuted: string;

  // Accent layer
  accent: string;
  onAccent: string;
  blue: string;
  cyan: string;
  green: string;
  yellow: string;
  red: string;
  purple: string;
  orange: string;

  // Diff layer
  addBg: string;
  addText: string;
  delBg: string;
  delText: string;
  hunkBg: string;
  /** Opaque background under a changed-word highlight box on an add line. */
  addChangedBg: string;
  /** Opaque background under a changed-word highlight box on a del line. */
  delChangedBg: string;

  // Interactive layer
  selectedBg: string;
}

/** Raw role tokens for one theme — hex strings mirror `theme-tokens.js`. */
interface ThemeTokens {
  name: string;
  light: boolean;
  syntaxThemeId: string;
  bg: string;
  bg1: string;
  bg2: string;
  bg3: string;
  /** Hairline border; may carry an 8-digit alpha suffix (e.g. `ffffff26`). */
  line2: string;
  tx: string;
  tx2: string;
  tx3: string;
  accent: string;
  onAccent: string;
  red: string;
  amber: string;
  blue: string;
  cyan: string;
  purple: string;
  green: string;
  add: string;
  del: string;
}

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16),
  ];
}

function rgbToHex([r, g, b]: [number, number, number]): string {
  return `#${[r, g, b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
}

/** Composite a (possibly 8-digit) hex `fg` over an opaque `bg`. */
function over(fg: string, bg: string): string {
  const h = fg.replace("#", "");
  const a = h.length >= 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1;
  const f = hexToRgb(h);
  const b = hexToRgb(bg);
  return rgbToHex([
    f[0] * a + b[0] * (1 - a),
    f[1] * a + b[1] * (1 - a),
    f[2] * a + b[2] * (1 - a),
  ]);
}

/** Composite `fg` at explicit opacity `t` over an opaque `bg`. */
function overAt(fg: string, t: number, bg: string): string {
  const f = hexToRgb(fg);
  const b = hexToRgb(bg);
  return rgbToHex([
    f[0] * t + b[0] * (1 - t),
    f[1] * t + b[1] * (1 - t),
    f[2] * t + b[2] * (1 - t),
  ]);
}

/**
 * Opacity of the intra-line "changed word" highlight box (`--color-wd-*-bg`),
 * composited over the add/del line background. Single-sourced here so the CSS
 * var, the derived `addChangedBg`/`delChangedBg` anchors, and the syntax
 * contrast corrector (`diffContrast.ts`) all agree on the same background.
 */
export const WORD_DIFF_HIGHLIGHT_ALPHA = 0.3;

/** Opacity of the add/del line tint composited over the canvas. */
const DIFF_LINE_ALPHA = 0.15;

function buildTheme(t: ThemeTokens): AppTheme {
  const addBg = overAt(t.add, DIFF_LINE_ALPHA, t.bg);
  const delBg = overAt(t.del, DIFF_LINE_ALPHA, t.bg);
  return {
    name: t.name,
    light: t.light,
    syntaxThemeId: t.syntaxThemeId,

    bg: t.bg,
    surface: t.bg1,
    panel: t.bg2,
    border: over(t.line2, t.bg),

    text: t.tx,
    textBright: t.tx,
    textDim: t.tx2,
    textMuted: t.tx3,

    accent: t.accent,
    onAccent: t.onAccent,
    blue: t.blue,
    cyan: t.cyan,
    green: t.green,
    yellow: t.amber,
    red: t.red,
    purple: t.purple,
    orange: t.accent,

    addBg,
    addText: t.add,
    delBg,
    delText: t.del,
    hunkBg: overAt(t.blue, 0.1, t.bg),
    addChangedBg: overAt(t.add, WORD_DIFF_HIGHLIGHT_ALPHA, addBg),
    delChangedBg: overAt(t.del, WORD_DIFF_HIGHLIGHT_ALPHA, delBg),

    selectedBg: t.bg3,
  };
}

const THEME_TOKENS: ThemeTokens[] = [
  // ── DARK ──────────────────────────────────────────────────────────────
  {
    name: "graphite",
    light: false,
    syntaxThemeId: "one-dark-pro",
    bg: "#0b0b0d", bg1: "#16161a", bg2: "#1d1d22", bg3: "#28282f",
    line2: "#ffffff26",
    tx: "#ededf0", tx2: "#a1a1ab", tx3: "#6b6b75",
    accent: "#f2843c", onAccent: "#1a0e05",
    red: "#ef5f5b", amber: "#e3b341", blue: "#5f9cea",
    cyan: "#4cc4e0", purple: "#a78bf6", green: "#46bd6c",
    add: "#46bd6c", del: "#ef5f5b",
  },
  {
    name: "slate",
    light: false,
    syntaxThemeId: "one-dark-pro",
    bg: "#0c1118", bg1: "#131b25", bg2: "#1a2431", bg3: "#243140",
    line2: "#9fc0ff2e",
    tx: "#e9eef5", tx2: "#9aa9bb", tx3: "#647588",
    accent: "#f2843c", onAccent: "#1a0e05",
    red: "#f0635f", amber: "#e6b84a", blue: "#6aa6f5",
    cyan: "#43c8e2", purple: "#ab92f7", green: "#4ec977",
    add: "#4ec977", del: "#f0635f",
  },
  {
    name: "midnight",
    light: false,
    syntaxThemeId: "tokyo-night",
    bg: "#0e0f1a", bg1: "#171829", bg2: "#1e2036", bg3: "#292c48",
    line2: "#a9b8ff2e",
    tx: "#e6e8f5", tx2: "#9aa0c4", tx3: "#666c90",
    accent: "#f2843c", onAccent: "#1a0e05",
    red: "#f7768e", amber: "#e0af68", blue: "#7aa2f7",
    cyan: "#7dcfff", purple: "#bb9af7", green: "#9ece6a",
    add: "#9ece6a", del: "#f7768e",
  },
  {
    name: "ember",
    light: false,
    syntaxThemeId: "one-dark-pro",
    bg: "#100c0a", bg1: "#1b1512", bg2: "#241c17", bg3: "#312620",
    line2: "#ffd9b82b",
    tx: "#f1e9e2", tx2: "#b09c8d", tx3: "#75665c",
    accent: "#f2843c", onAccent: "#1f0f04",
    red: "#ef6151", amber: "#e7b24a", blue: "#7aa6dd",
    cyan: "#56bfc0", purple: "#c193e8", green: "#76b95f",
    add: "#76b95f", del: "#ef6151",
  },

  // ── LIGHT ─────────────────────────────────────────────────────────────
  {
    name: "paper",
    light: true,
    syntaxThemeId: "one-light",
    bg: "#faf8f4", bg1: "#ffffff", bg2: "#f3efe8", bg3: "#e9e3d8",
    line2: "#3a2a1a2b",
    tx: "#211d18", tx2: "#6b6258", tx3: "#9a9085",
    accent: "#cf5f17", onAccent: "#ffffff",
    red: "#cc3b39", amber: "#a9740f", blue: "#2f6fd0",
    cyan: "#0e87a3", purple: "#7a4fd0", green: "#1c854b",
    add: "#1c854b", del: "#cc3b39",
  },
  {
    name: "daylight",
    light: true,
    syntaxThemeId: "one-light",
    bg: "#f6f7f9", bg1: "#ffffff", bg2: "#eef0f3", bg3: "#e3e7ec",
    line2: "#0b1b3a29",
    tx: "#161a21", tx2: "#5b6573", tx3: "#8b95a3",
    accent: "#cf5f17", onAccent: "#ffffff",
    red: "#cc3b39", amber: "#9a6b12", blue: "#2563cf",
    cyan: "#0b7e9c", purple: "#6f45cc", green: "#168049",
    add: "#168049", del: "#cc3b39",
  },

  // ── ACCESSIBILITY ───────────────────────────────────────────────────────
  {
    name: "contrast-dark",
    light: false,
    syntaxThemeId: "github-dark-high-contrast",
    bg: "#000000", bg1: "#0a0a0b", bg2: "#151517", bg3: "#202024",
    line2: "#ffffff5c",
    tx: "#ffffff", tx2: "#d4d4da", tx3: "#9a9aa2",
    accent: "#ff9a4d", onAccent: "#000000",
    red: "#ff6b66", amber: "#ffcf4d", blue: "#7fb4ff",
    cyan: "#5fd6f0", purple: "#cbabff", green: "#5fe08a",
    add: "#5fe08a", del: "#ff6b66",
  },
  {
    name: "contrast-light",
    light: true,
    syntaxThemeId: "github-light-high-contrast",
    bg: "#ffffff", bg1: "#ffffff", bg2: "#f2f2f4", bg3: "#e6e6ea",
    line2: "#0000005c",
    tx: "#000000", tx2: "#2e2e33", tx3: "#5a5a61",
    accent: "#b8530c", onAccent: "#ffffff",
    red: "#bf1b1b", amber: "#7a5300", blue: "#1551c4",
    cyan: "#056b85", purple: "#6321c0", green: "#0c7a3f",
    add: "#0c7a3f", del: "#bf1b1b",
  },
];

export const APP_THEMES: AppTheme[] = THEME_TOKENS.map(buildTheme);

export const DEFAULT_THEME_NAME = "graphite";

/** Backward-compat aliases for the retired theme set. */
const THEME_ALIASES: Record<string, string> = {
  "ocean-depth": "graphite",
  moonlight: "slate",
  "high-contrast": "contrast-dark",
  daybreak: "daylight",
  "tokyo-night": "midnight",
  "tokyo-night-storm": "midnight",
  "tokyo-night-moon": "midnight",
  "tokyo-night-day": "paper",
};

export function themeByName(name: string | null | undefined): AppTheme {
  const resolved = (name && THEME_ALIASES[name]) || name;
  return APP_THEMES.find((t) => t.name === resolved) ?? APP_THEMES[0];
}

function alpha(hex: string, a: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r},${g},${b},${a})`;
}

/** Linear blend of two hex colors; t=0 → a, t=1 → b. */
function mix(a: string, b: string, t: number): string {
  const ra = hexToRgb(a);
  const rb = hexToRgb(b);
  return rgbToHex([
    ra[0] + (rb[0] - ra[0]) * t,
    ra[1] + (rb[1] - ra[1]) * t,
    ra[2] + (rb[2] - ra[2]) * t,
  ]);
}

/** CSS custom-property overrides for a theme, keyed by the vars in app.css. */
export function cssVarsFor(t: AppTheme): Record<string, string> {
  return {
    // Ink ladder: bg → surface → panel → border → text, interpolated between anchors.
    "--color-ink-900": t.bg,
    "--color-ink-880": t.surface,
    "--color-ink-870": mix(t.surface, t.panel, 0.2),
    "--color-ink-850": mix(t.surface, t.panel, 0.5),
    "--color-ink-800": t.panel,
    "--color-ink-750": mix(t.panel, t.border, 0.2),
    "--color-ink-700": mix(t.panel, t.border, 0.4),
    "--color-ink-650": mix(t.panel, t.border, 0.6),
    "--color-ink-600": mix(t.panel, t.border, 0.8),
    "--color-ink-500": t.border,
    "--color-ink-400": mix(t.border, t.textMuted, 0.5),
    "--color-ink-300": t.textMuted,
    "--color-ink-200": t.textDim,
    "--color-ink-100": t.text,
    "--color-ink-50": t.textBright,

    "--color-accent": t.accent,
    "--color-accent-soft": alpha(t.accent, 0.12),
    "--color-accent-border": alpha(t.accent, 0.3),
    "--color-accent-hover": mix(t.accent, t.light ? "#000000" : "#ffffff", 0.12),
    "--color-on-accent": t.onAccent,
    "--color-periwinkle": t.purple,
    "--color-periwinkle-soft": alpha(t.purple, 0.14),

    // Accent ladder (semantic accents shared with the TUI palette).
    "--color-action": t.blue,
    "--color-info": t.cyan,
    "--color-success": t.green,
    "--color-warning": t.yellow,
    "--color-error": t.red,
    "--color-emphasis": t.orange,

    "--color-add-bg": t.addBg,
    "--color-add-fg": t.addText,
    "--color-del-bg": t.delBg,
    "--color-del-fg": t.delText,

    // Intra-line "changed word" highlight boxes (composited over the line bg).
    "--color-wd-add-bg": alpha(t.addText, WORD_DIFF_HIGHLIGHT_ALPHA),
    "--color-wd-del-bg": alpha(t.delText, WORD_DIFF_HIGHLIGHT_ALPHA),

    "--color-risk-high": t.red,
    "--color-risk-med": t.yellow,
    "--color-risk-low": t.blue,

    "--color-comment": t.cyan,
    "--color-question": t.yellow,
    "--color-question-surface": alpha(t.yellow, 0.04),
    "--color-question-border": alpha(t.yellow, 0.25),
    "--color-ai": t.yellow,

    "--color-tree-selected": alpha(t.blue, 0.14),
    "--color-finding": t.purple,

    "--color-row-select": alpha(t.blue, 0.18),
    "--color-row-select-weak": alpha(t.blue, 0.1),
    "--color-row-select-strong": alpha(t.blue, 0.22),
    "--color-row-select-edge": alpha(t.blue, 0.55),
    "--color-row-select-accent": t.blue,

    // Fallback CSS-side syntax colors (Shiki spans override these per token).
    "--color-syntax-keyword": t.purple,
    "--color-syntax-type": t.cyan,
    "--color-syntax-fn": t.yellow,
    "--color-syntax-param": t.orange,
    "--color-syntax-string": t.green,
    "--color-syntax-punct": t.textMuted,
    "--color-syntax-comment": t.textMuted,
  };
}

/** xterm.js theme derived from an app theme (embedded terminal drawer). */
export function xtermThemeFor(t: AppTheme): Record<string, string> {
  return {
    background: t.surface,
    foreground: t.text,
    cursor: t.orange,
    cursorAccent: t.surface,
    selectionBackground: t.selectedBg,
    black: t.panel,
    red: t.red,
    green: t.green,
    yellow: t.yellow,
    blue: t.blue,
    magenta: t.purple,
    cyan: t.cyan,
    white: t.text,
    brightBlack: t.textDim,
    brightRed: t.red,
    brightGreen: t.green,
    brightYellow: t.yellow,
    brightBlue: t.blue,
    brightMagenta: t.purple,
    brightCyan: t.cyan,
    brightWhite: t.textBright,
  };
}

let appliedThemeName: string | null = null;

/**
 * Apply a theme by name to the document root. Idempotent per name —
 * safe to call from a reactive effect on every snapshot.
 */
export function applyTheme(name: string | null | undefined): AppTheme {
  const theme = themeByName(name);
  if (appliedThemeName === theme.name) return theme;
  appliedThemeName = theme.name;

  if (typeof document === "undefined") return theme;
  const root = document.documentElement;
  for (const [key, value] of Object.entries(cssVarsFor(theme))) {
    root.style.setProperty(key, value);
  }
  root.style.colorScheme = theme.light ? "light" : "dark";
  return theme;
}

/** Syntax theme id for a TUI theme name (used to drive the Shiki worker). */
export function syntaxThemeIdForTheme(name: string | null | undefined): string {
  return themeByName(name)?.syntaxThemeId ?? DEFAULT_SYNTAX_THEME_ID;
}
