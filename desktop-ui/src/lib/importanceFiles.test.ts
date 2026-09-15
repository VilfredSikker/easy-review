import { describe, expect, it } from "bun:test";
import {
  IMPORTANCE_FILE_LIMIT,
  importanceFileWindow,
  matchedRuleLabel,
} from "./importanceFiles";
import type { ImportanceFileSnapshot } from "./types";

function file(path: string, tier: string, matchedRule: string | null): ImportanceFileSnapshot {
  return { path, tier, matchedRule };
}

function files(n: number): ImportanceFileSnapshot[] {
  return Array.from({ length: n }, (_, i) => file(`src/f${i}.rs`, "normal", null));
}

describe("importanceFileWindow", () => {
  it("shows a list shorter than the limit whole", () => {
    const rows = files(3);
    const { shown, hidden } = importanceFileWindow(rows);
    expect(shown).toEqual(rows);
    expect(hidden).toBe(0);
  });

  it("shows a list exactly at the limit whole", () => {
    // The boundary is where an off-by-one hides a row and reports nothing.
    const rows = files(IMPORTANCE_FILE_LIMIT);
    const { shown, hidden } = importanceFileWindow(rows);
    expect(shown.length).toBe(IMPORTANCE_FILE_LIMIT);
    expect(hidden).toBe(0);
  });

  it("counts what it left out rather than dropping it", () => {
    const rows = files(IMPORTANCE_FILE_LIMIT + 5);
    const { shown, hidden } = importanceFileWindow(rows);
    expect(shown.length).toBe(IMPORTANCE_FILE_LIMIT);
    expect(hidden).toBe(5);
    // Nothing is invented or lost between the two halves.
    expect(shown.length + hidden).toBe(rows.length);
  });

  it("resolves a file no rule claimed to the default's tier", () => {
    // The row is still drawn: "nothing matched" is an answer, and the tier it
    // carries is the one the resolver reported.
    const rows = [file("README.md", "isolated", null)];
    const { shown, hidden } = importanceFileWindow(rows);
    expect(hidden).toBe(0);
    expect(shown[0].tier).toBe("isolated");
  });
});

describe("matchedRuleLabel", () => {
  it("names the rule key that claimed the file", () => {
    expect(matchedRuleLabel(file("src/a.rs", "normal", "crates/**"))).toBe("crates/**");
  });

  it("says so plainly when no rule claimed it", () => {
    // An empty cell reads as missing data; the default is an answer.
    expect(matchedRuleLabel(file("README.md", "isolated", null))).toBe("no rule");
  });
});
