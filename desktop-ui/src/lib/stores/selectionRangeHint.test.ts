import { describe, expect, test } from "bun:test";
import { selectionRangeHint } from "./selectionRangeHint";

describe("selectionRangeHint", () => {
  test("a single line teaches shift-click", () => {
    expect(selectionRangeHint(5, 5)).toBe("shift-click another line for a range");
  });
  test("reports the span in either direction", () => {
    expect(selectionRangeHint(9, 6)).toBe("4 lines");
    expect(selectionRangeHint(6, 9)).toBe("4 lines");
  });
  test("no selection has no hint", () => {
    expect(selectionRangeHint(null, null)).toBe("");
  });
});
