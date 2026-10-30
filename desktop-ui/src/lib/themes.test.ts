import { describe, expect, it } from "bun:test";
import { APP_THEMES, commitSeriesFor, cssVarsFor, themeByName } from "./themes";

const lum = (hex: string) => {
  const n = parseInt(hex.slice(1), 16);
  return ((n >> 16) & 255) + ((n >> 8) & 255) + (n & 255);
};

describe("derived ground and line", () => {
  it("pins graphite to the values the Rust palette test pins (ADR 0030)", () => {
    const g = themeByName("graphite");
    expect(g.ground).toBe("#060607");
    expect(g.border).toBe("#2f2f31");
    expect(g.line).toBe("#1f1f21");
  });

  it("keeps line between bg and border and ground off the canvas on every theme", () => {
    for (const t of APP_THEMES) {
      const dLine = Math.abs(lum(t.line) - lum(t.bg));
      const dBorder = Math.abs(lum(t.border) - lum(t.bg));
      expect(dLine).toBeGreaterThan(0);
      expect(dLine).toBeLessThan(dBorder);
      // contrast-dark's canvas is already black, so ground can't go deeper.
      expect(t.ground === t.bg ? t.bg === "#000000" : true).toBe(true);
    }
  });
});

describe("kind colours", () => {
  it("gives comment, question, note and finding distinct colours on every theme", () => {
    for (const t of APP_THEMES) {
      const v = cssVarsFor(t);
      const kinds = [
        v["--color-comment"],
        v["--color-question"],
        v["--color-note"],
        v["--color-finding"],
      ];
      expect(new Set(kinds).size).toBe(4);
    }
  });

  it("uses the mapping the TUI pins in themes.rs (ADR 0030)", () => {
    for (const t of APP_THEMES) {
      const v = cssVarsFor(t);
      expect(v["--color-comment"]).toBe(t.blue);
      expect(v["--color-question"]).toBe(t.purple);
      expect(v["--color-note"]).toBe(t.yellow);
      expect(v["--color-finding"]).toBe(t.cyan);
    }
  });
});

describe("commit series", () => {
  it("has six distinct colours per theme, exposed as CSS vars", () => {
    for (const t of APP_THEMES) {
      const series = commitSeriesFor(t);
      expect(new Set(series).size).toBe(6);
      const v = cssVarsFor(t);
      series.forEach((c, i) => expect(v[`--color-commit-${i + 1}`]).toBe(c));
    }
  });
});
