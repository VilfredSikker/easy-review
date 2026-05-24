import { describe, expect, it } from "bun:test";
import {
  applyHunkSpansIfChanged,
  cacheWouldImproveFile,
  fileHasColoredSpans,
  fileNeedsSyntaxSpans,
  hasColoredSyntaxSpans,
  lineNeedsSyntaxSpans,
  shouldSkipHighlightApply,
  syntaxSpansEqual,
} from "./highlightPlan";
import type { HunkHighlight } from "./highlightCache";
import type { FileSnapshot, HunkSnapshot, LineSnapshot } from "./types";

function line(
  kind: LineSnapshot["kind"],
  text: string,
  spans?: LineSnapshot["spans"],
): LineSnapshot {
  return { kind, text, old_num: null, new_num: null, spans };
}

function file(hunks: HunkSnapshot[]): FileSnapshot {
  return {
    path: "src/foo.ts",
    hunks,
    additions: 0,
    deletions: 0,
    reviewed: false,
    source_index: 0,
    cache_key: "k",
    compacted: false,
    is_lazy_stub: false,
  };
}

describe("hasColoredSyntaxSpans", () => {
  it("is false for undefined, empty, or colorless spans", () => {
    expect(hasColoredSyntaxSpans(undefined)).toBe(false);
    expect(hasColoredSyntaxSpans([])).toBe(false);
    expect(hasColoredSyntaxSpans([{ text: "x", color: "" }])).toBe(false);
  });

  it("is true when any span has a color", () => {
    expect(hasColoredSyntaxSpans([{ text: "x", color: "#fff" }])).toBe(true);
  });
});

describe("lineNeedsSyntaxSpans", () => {
  it("ignores fold lines", () => {
    expect(lineNeedsSyntaxSpans(line("fold", "··· 5 lines ···"))).toBe(false);
  });

  it("is true without spans or colors", () => {
    expect(lineNeedsSyntaxSpans(line("add", "x"))).toBe(true);
    expect(lineNeedsSyntaxSpans(line("context", "x", [{ text: "x", color: "" }]))).toBe(
      true,
    );
  });

  it("is false when a colored span exists", () => {
    expect(
      lineNeedsSyntaxSpans(line("context", "x", [{ text: "x", color: "#fff" }])),
    ).toBe(false);
  });
});

describe("fileNeedsSyntaxSpans", () => {
  it("returns true when any non-fold line lacks colored spans", () => {
    const f = file([
      {
        header: "@@",
        lines: [
          { ...line("context", "a"), spans: [{ text: "a", color: "#fff" }] },
          line("add", "b"),
        ],
      },
    ]);
    expect(fileNeedsSyntaxSpans(f)).toBe(true);
    expect(fileHasColoredSpans(f)).toBe(true);
  });

  it("returns false when every line has colored spans", () => {
    const f = file([
      {
        header: "@@",
        lines: [{ ...line("add", "b"), spans: [{ text: "b", color: "#fff" }] }],
      },
    ]);
    expect(fileNeedsSyntaxSpans(f)).toBe(false);
    expect(fileHasColoredSpans(f)).toBe(true);
  });

  it("returns false for empty hunks", () => {
    const f = file([{ header: "@@", lines: [] }]);
    expect(fileNeedsSyntaxSpans(f)).toBe(false);
    expect(fileHasColoredSpans(f)).toBe(false);
  });
});

describe("syntaxSpansEqual", () => {
  it("compares token text and color", () => {
    const a = [{ text: "foo", color: "#a" }];
    const b = [{ text: "foo", color: "#a" }];
    const c = [{ text: "foo", color: "#b" }];
    expect(syntaxSpansEqual(a, b)).toBe(true);
    expect(syntaxSpansEqual(a, c)).toBe(false);
    expect(syntaxSpansEqual(undefined, undefined)).toBe(true);
  });
});

describe("applyHunkSpansIfChanged", () => {
  const colored: HunkHighlight[] = [
    {
      hunk_index: 0,
      lines: [[{ text: "x", color: "#fff" }], []],
    },
  ];

  it("returns false and does not replace hunks when spans are unchanged", () => {
    const f = file([
      {
        header: "@@",
        lines: [{ ...line("add", "x"), spans: [{ text: "x", color: "#fff" }] }, line("add", "y")],
      },
    ]);
    const hunksBefore = f.hunks;
    expect(applyHunkSpansIfChanged(f, colored)).toBe(false);
    expect(f.hunks).toBe(hunksBefore);
  });

  it("returns true and updates lines that gain colored spans", () => {
    const f = file([
      {
        header: "@@",
        lines: [line("add", "x"), line("add", "y")],
      },
    ]);
    expect(applyHunkSpansIfChanged(f, colored)).toBe(true);
    expect(f.hunks[0].lines[0].spans).toEqual([{ text: "x", color: "#fff" }]);
    expect(f.hunks[0].lines[1].spans).toBeUndefined();
  });
});

describe("cacheWouldImproveFile", () => {
  it("is false when cache matches live colored spans", () => {
    const f = file([
      {
        header: "@@",
        lines: [{ ...line("add", "x"), spans: [{ text: "x", color: "#fff" }] }],
      },
    ]);
    const hunks: HunkHighlight[] = [
      { hunk_index: 0, lines: [[{ text: "x", color: "#fff" }]] },
    ];
    expect(cacheWouldImproveFile(f, hunks)).toBe(false);
  });

  it("is true when a line would gain new colored spans", () => {
    const f = file([
      {
        header: "@@",
        lines: [line("add", "x")],
      },
    ]);
    const hunks: HunkHighlight[] = [
      { hunk_index: 0, lines: [[{ text: "x", color: "#fff" }]] },
    ];
    expect(cacheWouldImproveFile(f, hunks)).toBe(true);
  });
});

describe("shouldSkipHighlightApply", () => {
  it("does not skip when poll wiped spans but the key was already applied", () => {
    const f = file([
      {
        header: "@@",
        lines: [line("add", "b")],
      },
    ]);
    expect(shouldSkipHighlightApply(f, true)).toBe(false);
  });

  it("skips when spans are still present on the live file", () => {
    const f = file([
      {
        header: "@@",
        lines: [{ ...line("add", "b"), spans: [{ text: "b", color: "#fff" }] }],
      },
    ]);
    expect(shouldSkipHighlightApply(f, true)).toBe(true);
    expect(shouldSkipHighlightApply(f, false)).toBe(false);
  });
});
