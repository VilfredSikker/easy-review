import { describe, expect, it } from "bun:test";
import {
  buildRulerMarks,
  clampPopoverPosition,
  collectUsageLines,
  groupUsagesByFile,
  usagePreview,
  type UsageLine,
  type UsageSource,
} from "./referenceUsages";

const src = (rowIdx: number, filePath: string, lineNum: number | null, text: string): UsageSource => ({
  rowIdx,
  filePath,
  lineNum,
  text,
});

describe("collectUsageLines", () => {
  it("keeps only lines with whole-word matches, preserving order", () => {
    const sources = [
      src(0, "a.ts", 1, "const foo = 1;"),
      src(1, "a.ts", 2, "const foobar = foo;"),
      src(2, "b.ts", 5, "no match here"),
      src(3, "b.ts", 6, "foo(foo)"),
    ];
    const out = collectUsageLines(sources, "foo");
    expect(out.map((u) => u.rowIdx)).toEqual([0, 1, 3]);
    expect(out[0].ranges).toEqual([[6, 9]]);
    // `foobar` must not match; the trailing `foo` does.
    expect(out[1].ranges).toEqual([[15, 18]]);
    expect(out[2].ranges).toEqual([
      [0, 3],
      [4, 7],
    ]);
  });

  it("returns empty for an empty identifier or no matches", () => {
    expect(collectUsageLines([src(0, "a.ts", 1, "foo")], "")).toEqual([]);
    expect(collectUsageLines([src(0, "a.ts", 1, "bar")], "foo")).toEqual([]);
  });
});

describe("groupUsagesByFile", () => {
  const usage = (rowIdx: number, filePath: string): UsageLine => ({
    ...src(rowIdx, filePath, rowIdx, "foo"),
    ranges: [[0, 3]],
  });

  it("groups consecutive usages by file", () => {
    const grouped = groupUsagesByFile([usage(0, "a.ts"), usage(1, "a.ts"), usage(2, "b.ts")]);
    expect(grouped.groups.map((g) => g.filePath)).toEqual(["a.ts", "b.ts"]);
    expect(grouped.groups[0].usages.length).toBe(2);
    expect(grouped.groups[1].usages.length).toBe(1);
    expect(grouped.total).toBe(3);
    expect(grouped.shown).toBe(3);
  });

  it("caps usages across groups and reports total vs shown", () => {
    const usages = [usage(0, "a.ts"), usage(1, "a.ts"), usage(2, "b.ts"), usage(3, "c.ts")];
    const grouped = groupUsagesByFile(usages, 3);
    expect(grouped.shown).toBe(3);
    expect(grouped.total).toBe(4);
    // c.ts falls entirely past the cap and is omitted.
    expect(grouped.groups.map((g) => g.filePath)).toEqual(["a.ts", "b.ts"]);
  });

  it("handles empty input", () => {
    expect(groupUsagesByFile([])).toEqual({ groups: [], total: 0, shown: 0 });
  });
});

describe("usagePreview", () => {
  it("splits around the match and strips leading indentation", () => {
    expect(usagePreview("    const foo = 1;", [10, 13])).toEqual({
      prefix: "const ",
      match: "foo",
      suffix: " = 1;",
    });
  });

  it("left-truncates a long prefix so the match stays visible", () => {
    const text = "x".repeat(40) + "foo()";
    const p = usagePreview(text, [40, 43], 10);
    expect(p.prefix).toBe("…" + "x".repeat(9));
    expect(p.match).toBe("foo");
    expect(p.suffix).toBe("()");
  });

  it("truncates the suffix to fit the total budget", () => {
    const text = "foo" + "y".repeat(100);
    const p = usagePreview(text, [0, 3], 24, 20);
    expect(p.prefix).toBe("");
    expect(p.match).toBe("foo");
    expect(p.suffix.length).toBe(17);
    expect(p.suffix.endsWith("…")).toBe(true);
  });

  it("leaves short lines untouched", () => {
    expect(usagePreview("a foo b", [2, 5])).toEqual({ prefix: "a ", match: "foo", suffix: " b" });
  });
});

describe("buildRulerMarks", () => {
  it("maps content offsets to ruler pixels proportionally", () => {
    const marks = buildRulerMarks(
      [
        { rowIdx: 0, offsetPx: 0 },
        { rowIdx: 10, offsetPx: 5000 },
      ],
      10000,
      500,
    );
    expect(marks).toEqual([
      { rowIdx: 0, topPx: 0 },
      { rowIdx: 10, topPx: 250 },
    ]);
  });

  it("clamps the last mark inside the ruler", () => {
    const marks = buildRulerMarks([{ rowIdx: 9, offsetPx: 10000 }], 10000, 500, 3);
    expect(marks).toEqual([{ rowIdx: 9, topPx: 497 }]);
  });

  it("merges marks that would overlap", () => {
    const marks = buildRulerMarks(
      [
        { rowIdx: 0, offsetPx: 0 },
        { rowIdx: 1, offsetPx: 24 }, // ~1px on the ruler — collides with rowIdx 0
        { rowIdx: 2, offsetPx: 5000 },
      ],
      10000,
      500,
      3,
    );
    expect(marks.map((m) => m.rowIdx)).toEqual([0, 2]);
  });

  it("returns empty for degenerate geometry", () => {
    expect(buildRulerMarks([{ rowIdx: 0, offsetPx: 0 }], 0, 500)).toEqual([]);
    expect(buildRulerMarks([{ rowIdx: 0, offsetPx: 0 }], 1000, 0)).toEqual([]);
  });
});

describe("clampPopoverPosition", () => {
  it("returns the anchor when the box fits", () => {
    expect(clampPopoverPosition(100, 200, 300, 200, 1280, 800)).toEqual({ left: 100, top: 200 });
  });

  it("clamps against the right and bottom viewport edges", () => {
    expect(clampPopoverPosition(1200, 750, 300, 200, 1280, 800)).toEqual({
      left: 1280 - 300 - 8,
      top: 800 - 200 - 8,
    });
  });

  it("never goes above the padding minimum", () => {
    expect(clampPopoverPosition(-50, -50, 300, 200, 1280, 800)).toEqual({ left: 8, top: 8 });
  });

  it("prefers the top-left edge when the box cannot fit", () => {
    expect(clampPopoverPosition(100, 100, 2000, 2000, 1280, 800)).toEqual({ left: 8, top: 8 });
  });
});
