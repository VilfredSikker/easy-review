import { describe, expect, it } from "bun:test";
import {
  contrastRatio,
  correctSyntaxColor,
  diffBgKind,
  MIN_DIFF_CONTRAST,
  type DiffBgKind,
} from "./diffContrast";
import { APP_THEMES, themeByName, type AppTheme } from "./themes";

/**
 * Representative token colors per Shiki theme, lifted verbatim from the bundled
 * `@shikijs/themes/*` JSON (comment / string / keyword / function / variable /
 * type / constant). These are the tones that land on diff backgrounds; the
 * audit asserts every one clears WCAG AA after correction, on every app theme.
 */
const SYNTAX_TOKENS: Record<string, Record<string, string>> = {
  "one-dark-pro": {
    comment: "#5c6370",
    string: "#98c379",
    keyword: "#c678dd",
    function: "#61afef",
    variable: "#e06c75",
    type: "#e5c07b",
    constant: "#d19a66",
  },
  "one-light": {
    comment: "#A0A1A7",
    string: "#50A14F",
    keyword: "#A626A4",
    function: "#4078F2",
    variable: "#E45649",
    type: "#C18401",
    constant: "#986801",
  },
  "tokyo-night": {
    comment: "#51597d",
    string: "#9ece6a",
    keyword: "#bb9af7",
    function: "#7aa2f7",
    variable: "#c0caf5",
    type: "#73daca",
    constant: "#ff9e64",
  },
  "github-dark-high-contrast": {
    comment: "#bdc4cc",
    string: "#addcff",
    keyword: "#ff9492",
    function: "#dbb7ff",
    variable: "#ffb757",
    type: "#72f088",
    constant: "#91cbff",
  },
  "github-light-high-contrast": {
    comment: "#66707b",
    string: "#032563",
    keyword: "#a0111f",
    function: "#622cbc",
    variable: "#702c00",
    type: "#024c1a",
    constant: "#023b95",
  },
};

function bgFor(theme: AppTheme, kind: DiffBgKind, changed: boolean): string {
  if (kind === "add") return changed ? theme.addChangedBg : theme.addBg;
  return changed ? theme.delChangedBg : theme.delBg;
}

const CASES: Array<{ kind: DiffBgKind; changed: boolean }> = [
  { kind: "add", changed: false },
  { kind: "add", changed: true }, // the changed-word highlight box (worst case)
  { kind: "del", changed: false },
  { kind: "del", changed: true },
];

describe("diffContrast — a11y audit across themes/tokens/backgrounds", () => {
  for (const theme of APP_THEMES) {
    const tokens = SYNTAX_TOKENS[theme.syntaxThemeId];
    for (const [tokenName, color] of Object.entries(tokens)) {
      for (const { kind, changed } of CASES) {
        const label = `${theme.name} · ${tokenName} on ${kind}${changed ? "/changed" : ""}`;
        it(`meets WCAG AA: ${label}`, () => {
          const corrected = correctSyntaxColor(color, theme, kind, changed)!;
          const bg = bgFor(theme, kind, changed);
          expect(contrastRatio(corrected, bg)).toBeGreaterThanOrEqual(MIN_DIFF_CONTRAST);
        });
      }
    }
  }
});

describe("diffContrast — behavior", () => {
  const daylight = themeByName("daylight");
  const graphite = themeByName("graphite");

  it("fixes the daylight comment-on-changed-add regression", () => {
    // The reported case: one-light comment gray on the green changed-word box.
    const raw = "#A0A1A7";
    expect(contrastRatio(raw, daylight.addChangedBg)).toBeLessThan(2); // was ~1.4:1
    const fixed = correctSyntaxColor(raw, daylight, "add", true)!;
    expect(contrastRatio(fixed, daylight.addChangedBg)).toBeGreaterThanOrEqual(MIN_DIFF_CONTRAST);
  });

  it("leaves already-legible colors untouched", () => {
    // one-dark-pro type yellow already clears AA on graphite's add bg.
    const raw = "#e5c07b";
    expect(contrastRatio(raw, graphite.addBg)).toBeGreaterThanOrEqual(MIN_DIFF_CONTRAST);
    expect(correctSyntaxColor(raw, graphite, "add", false)).toBe(raw);
  });

  it("does not correct context lines (native syntax palette preserved)", () => {
    const raw = "#A0A1A7";
    expect(correctSyntaxColor(raw, daylight, "context", false)).toBe(raw);
  });

  it("preserves hue while darkening on light themes", () => {
    // Function blue stays blue-dominant after darkening, not muddied to gray.
    const fixed = correctSyntaxColor("#4078F2", daylight, "add", true)!;
    const n = parseInt(fixed.slice(1), 16);
    const [r, g, b] = [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
    expect(b).toBeGreaterThan(r);
    expect(b).toBeGreaterThan(g);
  });

  it("passes through empty/undefined colors", () => {
    expect(correctSyntaxColor(undefined, daylight, "add", true)).toBeUndefined();
    expect(correctSyntaxColor("", daylight, "add", true)).toBe("");
  });

  it("diffBgKind maps add/del through and folds everything else to context", () => {
    expect(diffBgKind("add")).toBe("add");
    expect(diffBgKind("del")).toBe("del");
    expect(diffBgKind("context")).toBe("context");
    expect(diffBgKind("fold")).toBe("context");
  });
});
