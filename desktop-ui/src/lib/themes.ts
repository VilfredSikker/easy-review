/**
 * Desktop port of the TUI theme palettes (crates/er-tui/src/ui/themes.rs).
 *
 * Each theme overrides the CSS custom properties declared in `app.css`
 * (`@theme` block) on the document root, so every Tailwind utility and
 * semantic alias (`--color-bg`, `--color-card`, …) follows the active theme.
 * The `--color-ink-*` ladder is interpolated from the theme's four anchor
 * surfaces (bg → surface → panel → border) and text colors so intermediate
 * shades stay consistent for light and dark palettes alike.
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

  // Interactive layer
  selectedBg: string;
}

export const APP_THEMES: AppTheme[] = [
  {
    name: "ocean-depth",
    light: false,
    syntaxThemeId: "one-dark-pro",
    bg: "#0b0b0f",
    surface: "#13131a",
    panel: "#1a1a24",
    border: "#2a2a3a",
    text: "#e4e4ef",
    textBright: "#e8e8f2",
    textDim: "#8888a0",
    textMuted: "#55556a",
    blue: "#60a5fa",
    cyan: "#22d3ee",
    green: "#4ade80",
    yellow: "#facc15",
    red: "#f87171",
    purple: "#a78bfa",
    orange: "#fb923c",
    addBg: "#0e1b17",
    addText: "#4ade80",
    delBg: "#1f0e14",
    delText: "#f87171",
    hunkBg: "#16162a",
    selectedBg: "#1e1830",
  },
  {
    name: "moonlight",
    light: false,
    syntaxThemeId: "one-dark-pro",
    bg: "#0e0e12",
    surface: "#16161c",
    panel: "#1e1e26",
    border: "#32323e",
    text: "#d2d2dc",
    textBright: "#dcdce6",
    textDim: "#78788c",
    textMuted: "#4e4e5f",
    blue: "#6e9bdc",
    cyan: "#50b9c8",
    green: "#64c382",
    yellow: "#d2b43c",
    red: "#d27878",
    purple: "#a08cd2",
    orange: "#d28c50",
    addBg: "#111b17",
    addText: "#64c382",
    delBg: "#1e1115",
    delText: "#d27878",
    hunkBg: "#181828",
    selectedBg: "#1e1a2c",
  },
  {
    name: "daybreak",
    light: true,
    syntaxThemeId: "one-light",
    bg: "#fafafc",
    surface: "#f2f2f6",
    panel: "#eaeaf0",
    border: "#c8c8d7",
    text: "#1e1e28",
    textBright: "#0f0f19",
    textDim: "#646478",
    textMuted: "#9494a5",
    blue: "#2563eb",
    cyan: "#0694a2",
    green: "#16a34a",
    yellow: "#a17804",
    red: "#dc2626",
    purple: "#7c3aed",
    orange: "#c2580e",
    addBg: "#e6fbee",
    addText: "#16a34a",
    delBg: "#fdeaeb",
    delText: "#dc2626",
    hunkBg: "#e2e8f8",
    selectedBg: "#e6e1f5",
  },
  {
    name: "high-contrast",
    light: false,
    syntaxThemeId: "github-dark-high-contrast",
    bg: "#000000",
    surface: "#0a0a0a",
    panel: "#141414",
    border: "#505050",
    text: "#ffffff",
    textBright: "#ffffff",
    textDim: "#b4b4b4",
    textMuted: "#787878",
    blue: "#0078ff",
    cyan: "#00e6ff",
    green: "#00ff50",
    yellow: "#ffdc00",
    red: "#ff3232",
    purple: "#be82ff",
    orange: "#ff8c00",
    addBg: "#001a0d",
    addText: "#00ff50",
    delBg: "#210000",
    delText: "#ff3232",
    hunkBg: "#000028",
    selectedBg: "#1e1432",
  },
  {
    name: "tokyo-night",
    light: false,
    syntaxThemeId: "tokyo-night",
    bg: "#1a1b26",
    surface: "#16161e",
    panel: "#1f2335",
    border: "#292e42",
    text: "#a9b1d6",
    textBright: "#c0caf5",
    textDim: "#545c7e",
    textMuted: "#565f89",
    blue: "#7aa2f7",
    cyan: "#7dcfff",
    green: "#9ece6a",
    yellow: "#e0af68",
    red: "#f7768e",
    purple: "#bb9af7",
    orange: "#ff9e64",
    addBg: "#21323d",
    addText: "#9ece6a",
    delBg: "#39232c",
    delText: "#f7768e",
    hunkBg: "#1f2231",
    selectedBg: "#292e42",
  },
  {
    name: "tokyo-night-storm",
    light: false,
    syntaxThemeId: "tokyo-night",
    bg: "#24283b",
    surface: "#1f2335",
    panel: "#1f2335",
    border: "#292e42",
    text: "#a9b1d6",
    textBright: "#c0caf5",
    textDim: "#545c7e",
    textMuted: "#565f89",
    blue: "#7aa2f7",
    cyan: "#7dcfff",
    green: "#9ece6a",
    yellow: "#e0af68",
    red: "#f7768e",
    purple: "#bb9af7",
    orange: "#ff9e64",
    addBg: "#293d4f",
    addText: "#9ece6a",
    delBg: "#422e3e",
    delText: "#f7768e",
    hunkBg: "#272d43",
    selectedBg: "#292e42",
  },
  {
    name: "tokyo-night-moon",
    light: false,
    syntaxThemeId: "tokyo-night",
    bg: "#222436",
    surface: "#1e2030",
    panel: "#1e2030",
    border: "#2f334d",
    text: "#828bb8",
    textBright: "#c8d3f5",
    textDim: "#636da6",
    textMuted: "#636da6",
    blue: "#82aaff",
    cyan: "#86e1fc",
    green: "#c3e88d",
    yellow: "#ffc777",
    red: "#ff757f",
    purple: "#c099ff",
    orange: "#ff966c",
    addBg: "#27394b",
    addText: "#c3e88d",
    delBg: "#3d283b",
    delText: "#ff757f",
    hunkBg: "#252a3f",
    selectedBg: "#2f334d",
  },
  {
    name: "tokyo-night-day",
    light: true,
    syntaxThemeId: "one-light",
    bg: "#e1e2e7",
    surface: "#d0d5e3",
    panel: "#d0d5e3",
    border: "#c4c8da",
    text: "#3760bf",
    textBright: "#343a4f",
    textDim: "#8990b3",
    textMuted: "#848cb5",
    blue: "#2e7de9",
    cyan: "#007197",
    green: "#587539",
    yellow: "#8c6c3e",
    red: "#f52a65",
    purple: "#9854f1",
    orange: "#b15c00",
    addBg: "#c6d5db",
    addText: "#587539",
    delBg: "#dcc8cc",
    delText: "#f52a65",
    hunkBg: "#d5d9e4",
    selectedBg: "#c4c8da",
  },
];

export const DEFAULT_THEME_NAME = "ocean-depth";

export function themeByName(name: string | null | undefined): AppTheme {
  return APP_THEMES.find((t) => t.name === name) ?? APP_THEMES[0];
}

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16),
  ];
}

/** Linear blend of two hex colors; t=0 → a, t=1 → b. */
function mix(a: string, b: string, t: number): string {
  const ra = hexToRgb(a);
  const rb = hexToRgb(b);
  const c = ra.map((v, i) => Math.round(v + (rb[i] - v) * t));
  return `#${c.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

function alpha(hex: string, a: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r},${g},${b},${a})`;
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

    "--color-accent": t.orange,
    "--color-accent-soft": alpha(t.orange, 0.12),
    "--color-accent-border": alpha(t.orange, 0.3),
    "--color-periwinkle": t.purple,
    "--color-periwinkle-soft": alpha(t.purple, 0.14),

    "--color-add-bg": t.addBg,
    "--color-add-fg": t.addText,
    "--color-del-bg": t.delBg,
    "--color-del-fg": t.delText,

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
