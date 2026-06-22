/**
 * Keep syntax token colors readable on diff add/del backgrounds.
 *
 * Shiki tokenizes against its own editor background, but the diff view renders
 * those tokens over the app theme's tinted add/del line backgrounds — and, for
 * changed words, over an even darker/saturated highlight box. Muted tones
 * (comments) and saturated tones (strings, keywords) routinely drop below a
 * legible contrast there, worst of all on light themes where a faint gray
 * comment over a green "changed word" box measures ~1.4:1 (see the daylight
 * report that motivated this).
 *
 * Rather than the old hand-maintained hex→hex table (which only covered
 * one-dark-pro and never touched light themes), this nudges any token color
 * toward black or white — hue preserved — only as far as needed to clear the
 * WCAG AA threshold against the actual background it lands on. Colors that
 * already pass are returned untouched, so the native palette survives wherever
 * it is already readable.
 */
import type { AppTheme } from "./themes";

export type DiffBgKind = "add" | "del" | "context";

/** WCAG AA contrast for normal-size text. Diff code is not "large text". */
const TARGET_CONTRAST = 4.5;

/**
 * sRGB luminance at which pure black and pure white yield equal contrast — the
 * principled crossover for deciding whether to darken or lighten a token.
 */
const DARKEN_ABOVE = 0.179;

type Rgb = [number, number, number];

function parseHex(hex: string): Rgb | null {
  if (!hex || hex[0] !== "#") return null;
  let h = hex.slice(1);
  if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
  if (h.length === 8) h = h.slice(0, 6); // ignore token alpha
  if (h.length !== 6) return null;
  const n = parseInt(h, 16);
  if (Number.isNaN(n)) return null;
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function toHex([r, g, b]: Rgb): string {
  const c = (v: number) =>
    Math.round(Math.max(0, Math.min(255, v)))
      .toString(16)
      .padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

function channelLin(v: number): number {
  const s = v / 255;
  return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

function relLuminance([r, g, b]: Rgb): number {
  return 0.2126 * channelLin(r) + 0.7152 * channelLin(g) + 0.0722 * channelLin(b);
}

function contrast(a: Rgb, b: Rgb): number {
  const la = relLuminance(a);
  const lb = relLuminance(b);
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

/** Contrast ratio between two hex colors (exported for the a11y audit test). */
export function contrastRatio(a: string, b: string): number {
  const ra = parseHex(a);
  const rb = parseHex(b);
  if (!ra || !rb) return 1;
  return contrast(ra, rb);
}

/**
 * Move `fg` toward black (light bg) or white (dark bg), preserving hue, only as
 * far as needed to reach `target` contrast against `bg`. Binary search on the
 * blend amount; the extremes (black/white) clear AA on every diff background we
 * ship, so this always converges.
 */
function nudgeToContrast(fg: Rgb, bg: Rgb, target: number): Rgb {
  if (contrast(fg, bg) >= target) return fg;
  const darken = relLuminance(bg) >= DARKEN_ABOVE;
  // Round inside the search so contrast is measured on the integer color that
  // is actually emitted — otherwise rounding can drop the result below target.
  const blend = (t: number): Rgb =>
    darken
      ? [Math.round(fg[0] * (1 - t)), Math.round(fg[1] * (1 - t)), Math.round(fg[2] * (1 - t))]
      : [
          Math.round(fg[0] + (255 - fg[0]) * t),
          Math.round(fg[1] + (255 - fg[1]) * t),
          Math.round(fg[2] + (255 - fg[2]) * t),
        ];

  let lo = 0;
  let hi = 1;
  // The extreme (black/white) clears AA on every diff background we ship.
  let best = darken ? ([0, 0, 0] as Rgb) : ([255, 255, 255] as Rgb);
  for (let i = 0; i < 22; i++) {
    const t = (lo + hi) / 2;
    const cand = blend(t);
    if (contrast(cand, bg) >= target) {
      best = cand;
      hi = t;
    } else {
      lo = t;
    }
  }
  return best;
}

function backgroundFor(theme: AppTheme, kind: DiffBgKind, changed: boolean): string | null {
  if (kind === "add") return changed ? theme.addChangedBg : theme.addBg;
  if (kind === "del") return changed ? theme.delChangedBg : theme.delBg;
  return null;
}

const cache = new Map<string, string>();

/**
 * Readability-corrected color for a syntax token rendered on a diff line.
 *
 * `context` lines (and lines with no diff tint) are left as-is — the native
 * syntax palette is calibrated for them. Only add/del backgrounds, and the
 * darker changed-word box on top of them, get corrected.
 */
export function correctSyntaxColor(
  color: string | undefined,
  theme: AppTheme,
  kind: DiffBgKind,
  changed: boolean,
): string | undefined {
  if (!color) return color;
  const bgHex = backgroundFor(theme, kind, changed);
  if (!bgHex) return color;

  const key = `${color}|${theme.name}|${kind}|${changed ? 1 : 0}`;
  const hit = cache.get(key);
  if (hit !== undefined) return hit;

  const fg = parseHex(color);
  const bg = parseHex(bgHex);
  const out = fg && bg ? toHex(nudgeToContrast(fg, bg, TARGET_CONTRAST)) : color;
  cache.set(key, out);
  return out;
}

/** Minimum contrast this module guarantees (exported for tests). */
export const MIN_DIFF_CONTRAST = TARGET_CONTRAST;
