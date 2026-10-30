import { describe, expect, test } from "bun:test";
import { contrastRatio } from "./diffContrast";
import { prCiState, prNeedsMyReview, projectColorIndex, projectColorVar, projectMonogram, projectTileInk } from "./sidebarModel";
import { APP_THEMES, commitSeriesFor } from "./themes";

describe("projectMonogram", () => {
  test("uppercases the first letter", () => expect(projectMonogram("easy-review")).toBe("E"));
  test("falls back for empty names", () => expect(projectMonogram("  ")).toBe("?"));
});

describe("projectColorVar", () => {
  test("is stable per name and a theme token", () => {
    expect(projectColorVar("discovery")).toBe(projectColorVar("discovery"));
    expect(projectColorVar("discovery")).toMatch(/^var\(--color-commit-[1-6]\)$/);
  });
});

describe("projectTileInk", () => {
  test("monogram clears 4.5:1 on every series swatch in every theme", () => {
    // One name per series slot, so every swatch is exercised.
    const names = new Map<number, string>();
    for (let i = 0; names.size < 6; i++) names.set(projectColorIndex(`p${i}`), `p${i}`);
    for (const theme of APP_THEMES) {
      for (const [slot, name] of names) {
        const swatch = commitSeriesFor(theme)[slot];
        expect(contrastRatio(projectTileInk(theme, name), swatch)).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});

describe("prCiState", () => {
  test("maps checks", () => {
    expect(prCiState({ checks_state: "PASSING" })).toBe("ok");
    expect(prCiState({ checks_state: "FAILING" })).toBe("fail");
    expect(prCiState({ checks_state: "PENDING" })).toBe("pend");
    expect(prCiState({ checks_state: null })).toBeNull();
  });
});

describe("prNeedsMyReview", () => {
  const open = { state: "OPEN" as const, is_draft: false, approved_by_me: false };
  test("open non-draft in queue", () => expect(prNeedsMyReview(open, true)).toBe(true));
  test("not in queue", () => expect(prNeedsMyReview(open, false)).toBe(false));
  test("draft or approved", () => {
    expect(prNeedsMyReview({ ...open, is_draft: true }, true)).toBe(false);
    expect(prNeedsMyReview({ ...open, approved_by_me: true }, true)).toBe(false);
  });
});
