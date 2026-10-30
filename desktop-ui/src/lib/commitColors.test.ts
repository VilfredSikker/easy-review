import { describe, expect, it } from "bun:test";
import {
  COMMIT_COLOR_TOKENS,
  commitColorToken,
  commitColorVar,
  commitIndexForFile,
} from "./commitColors";

describe("commitColorToken", () => {
  it("maps index to a theme token in order", () => {
    expect(commitColorToken(0)).toBe(COMMIT_COLOR_TOKENS[0]);
    expect(commitColorToken(1)).toBe(COMMIT_COLOR_TOKENS[1]);
  });

  it("wraps past the palette and tolerates negatives and NaN", () => {
    const n = COMMIT_COLOR_TOKENS.length;
    expect(commitColorToken(n)).toBe(COMMIT_COLOR_TOKENS[0]);
    expect(commitColorToken(-1)).toBe(COMMIT_COLOR_TOKENS[n - 1]);
    expect(commitColorToken(Number.NaN)).toBe(COMMIT_COLOR_TOKENS[0]);
  });

  it("never emits raw hex", () => {
    for (const t of COMMIT_COLOR_TOKENS) expect(t.startsWith("--color-")).toBe(true);
    expect(commitColorVar(0)).toBe(`var(${COMMIT_COLOR_TOKENS[0]})`);
  });
});

describe("commitIndexForFile", () => {
  const commits = [{ files: ["a", "b"] }, { files: ["c"] }];
  it("finds the first commit touching the file", () => {
    expect(commitIndexForFile(commits, "c")).toBe(1);
    expect(commitIndexForFile(commits, "a")).toBe(0);
  });
  it("is -1 when no commit touches it", () => {
    expect(commitIndexForFile(commits, "z")).toBe(-1);
  });
});
