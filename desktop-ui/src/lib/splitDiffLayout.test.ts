import { describe, expect, it } from "bun:test";
import { gutterWidthPx, maxLineNumber, SPLIT_GUTTER_PX } from "./splitDiffLayout";
import type { FileSnapshot } from "./types";

function file(hunks: [oldStart: number, oldCount: number, newStart: number, newCount: number][]) {
  return {
    hunks: hunks.map(([old_start, old_count, new_start, new_count]) => ({ old_start, old_count, new_start, new_count })),
  } as unknown as FileSnapshot;
}

describe("maxLineNumber", () => {
  it("takes the last line of the furthest hunk on either side", () => {
    expect(maxLineNumber([file([[1, 5, 1, 5]]), file([[8620, 10, 8631, 21]])])).toBe(8651);
    expect(maxLineNumber([file([[10620, 4, 900, 2]])])).toBe(10623);
  });

  it("is 0 with no hunks", () => {
    expect(maxLineNumber([])).toBe(0);
    expect(maxLineNumber([file([])])).toBe(0);
  });
});

describe("gutterWidthPx", () => {
  const CHAR = 7.8;

  it("keeps the default width up to three digits", () => {
    expect(gutterWidthPx(0, CHAR)).toBe(SPLIT_GUTTER_PX);
    expect(gutterWidthPx(999, CHAR)).toBe(SPLIT_GUTTER_PX);
  });

  it("widens so four and five digits clear the accent bar", () => {
    const four = gutterWidthPx(3029, CHAR);
    const five = gutterWidthPx(10626, CHAR);
    expect(four).toBeGreaterThan(SPLIT_GUTTER_PX);
    expect(five).toBeGreaterThan(four);
    // Digits plus the right pad never reach the 3px accent bar on the left.
    expect(five - 8 - 5 * CHAR).toBeGreaterThanOrEqual(6);
  });

  it("falls back to a default char width before the probe measures", () => {
    expect(gutterWidthPx(10626, 0)).toBeGreaterThan(SPLIT_GUTTER_PX);
  });
});
