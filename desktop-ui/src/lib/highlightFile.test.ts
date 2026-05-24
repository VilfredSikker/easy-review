import { describe, expect, it } from "bun:test";
import {
  buildHighlightSides,
  fileNeedsSyntaxSpans,
  spansToHunksFromSides,
} from "./highlightPlan";
import type { FileSnapshot, HunkSnapshot, LineSnapshot } from "./types";

function line(
  kind: LineSnapshot["kind"],
  text: string,
): LineSnapshot {
  return { kind, text, old_num: null, new_num: null };
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

describe("buildHighlightSides", () => {
  it("skips deletions in new-side stitch and additions in old-side stitch", () => {
    const f = file([
      {
        header: "@@",
        lines: [
          line("context", "ctx"),
          line("del", "old"),
          line("add", "new"),
        ],
      },
    ]);
    const { newSide, oldSide } = buildHighlightSides(f);
    expect(newSide.texts).toEqual(["ctx", "new"]);
    expect(oldSide.texts).toEqual(["ctx", "old"]);
    expect(newSide.refs).toEqual([
      { hunkIdx: 0, lineIdx: 0 },
      { hunkIdx: 0, lineIdx: 2 },
    ]);
    expect(oldSide.refs).toEqual([
      { hunkIdx: 0, lineIdx: 0 },
      { hunkIdx: 0, lineIdx: 1 },
    ]);
  });
});

describe("spansToHunksFromSides", () => {
  it("assigns new-side spans to add/context and old-side spans to del", () => {
    const f = file([
      {
        header: "@@",
        lines: [
          line("context", "ctx"),
          line("del", "old"),
          line("add", "new"),
        ],
      },
    ]);
    const sides = buildHighlightSides(f);
    const hunks = spansToHunksFromSides(
      f,
      sides.newSide,
      [[{ text: "ctx", color: "#a" }], [{ text: "new", color: "#b" }]],
      sides.oldSide,
      [[{ text: "ctx", color: "#a" }], [{ text: "old", color: "#c" }]],
    );
    expect(hunks[0].lines[0]).toEqual([{ text: "ctx", color: "#a" }]);
    expect(hunks[0].lines[1]).toEqual([{ text: "old", color: "#c" }]);
    expect(hunks[0].lines[2]).toEqual([{ text: "new", color: "#b" }]);
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
  });

  it("returns false when every line has colored spans", () => {
    const f = file([
      {
        header: "@@",
        lines: [
          { ...line("add", "b"), spans: [{ text: "b", color: "#fff" }] },
        ],
      },
    ]);
    expect(fileNeedsSyntaxSpans(f)).toBe(false);
  });
});
