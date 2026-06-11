import { describe, expect, it } from "bun:test";
import {
  buildRulerMarks,
  clampPopoverPosition,
  collectMatches,
  collectUsageLines,
  groupUsagesByFile,
  usagePreview,
  type UsageLine,
  type UsageSource,
} from "./referenceUsages";
import { IDENTIFIER_MATCH_OPTIONS } from "./referenceHighlight";

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

describe("collectMatches", () => {
  const substring = { wordBoundary: false, caseSensitive: false };

  it("counts individual ranges across lines", () => {
    const sources = [
      src(0, "a.ts", 1, "foo(foo)"), // 2 ranges
      src(1, "a.ts", 2, "no match"),
      src(2, "b.ts", 3, "foobar"), // 1 range (substring mode)
    ];
    const out = collectMatches(sources, "foo", substring);
    expect(out.lines.map((l) => l.rowIdx)).toEqual([0, 2]);
    expect(out.total).toBe(3);
    expect(out.capped).toBe(false);
  });

  it("matches a full path query in substring mode", () => {
    const sources = [
      src(0, "api.ts", 1, 'route("/experiments/{experiment_id}/quality-control/wells")'),
      src(1, "api.ts", 2, "unrelated line"),
    ];
    const out = collectMatches(
      sources,
      "/experiments/{experiment_id}/quality-control/wells",
      substring,
    );
    expect(out.lines.map((l) => l.rowIdx)).toEqual([0]);
    expect(out.total).toBe(1);
  });

  it("stops at the cap, reports capped, and total equals the cap", () => {
    const sources = Array.from({ length: 10 }, (_, i) => src(i, "a.ts", i + 1, "x x x"));
    // 10 lines × 3 ranges = 30 potential matches; cap at 7 (mid-line).
    const out = collectMatches(sources, "x", substring, 7);
    expect(out.capped).toBe(true);
    expect(out.total).toBe(7);
    const ranges = out.lines.reduce((n, l) => n + l.ranges.length, 0);
    expect(ranges).toBe(7);
    // Collection stopped: only 3 lines were materialized (2 full + 1 trimmed).
    expect(out.lines.length).toBe(3);
    expect(out.lines[2].ranges.length).toBe(1);
  });

  it("does not report capped when matches exactly exhaust the sources", () => {
    const sources = [src(0, "a.ts", 1, "x x")];
    const out = collectMatches(sources, "x", substring, 2);
    expect(out.total).toBe(2);
    expect(out.capped).toBe(false);
  });

  it("matches collectUsageLines in identifier mode without a cap", () => {
    const sources = [
      src(0, "a.ts", 1, "const foo = 1;"),
      src(1, "a.ts", 2, "const foobar = foo;"),
      src(3, "b.ts", 6, "foo(foo)"),
    ];
    const viaMatches = collectMatches(sources, "foo", IDENTIFIER_MATCH_OPTIONS, Infinity);
    expect(viaMatches.lines).toEqual(collectUsageLines(sources, "foo"));
    expect(viaMatches.total).toBe(4);
    expect(viaMatches.capped).toBe(false);
  });

  it("returns empty for an empty query", () => {
    expect(collectMatches([src(0, "a.ts", 1, "foo")], "", substring)).toEqual({
      lines: [],
      total: 0,
      capped: false,
    });
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
