import { describe, expect, it } from "bun:test";
import type { CrossFileFlatRow } from "./diffRenderModel";
import type { DocumentBlock } from "./documentPreview";
import { sideBlockPadding, sideBlockSpans } from "./documentSideLayout";
import type { FileSnapshot, LineSnapshot } from "./types";

const PATH = "doc.md";

function block(startLine: number, endLine: number): DocumentBlock {
  return { startLine, endLine, segment: { kind: "html", html: `<p>${startLine}</p>` } };
}

/** Rows for one hunk: a header, then one content row per line, with a thread
 *  row after any line listed in `threadsAfter`. */
function fixture(lines: Pick<LineSnapshot, "kind" | "old_num" | "new_num">[], threadsAfter: number[] = []) {
  const file = {
    path: PATH,
    hunks: [{ lines: lines.map((l) => ({ ...l, text: "" })) }],
  } as unknown as FileSnapshot;
  const rows: CrossFileFlatRow[] = [
    { type: "file-header", filePath: PATH, fileIndex: 0, sourceIndex: 0, height: 30, identity: "fh", additions: 0, deletions: 0 },
    { type: "hunk-header", filePath: PATH, hunkIdx: 0, header: "@@", height: 20, identity: "hh" },
  ];
  lines.forEach((_, lineIdx) => {
    rows.push({ type: "content-unified", filePath: PATH, hunkIdx: 0, lineIdx, height: 20, identity: `cu${lineIdx}` });
    if (threadsAfter.includes(lineIdx)) {
      rows.push({ type: "inline-thread", filePath: PATH, hunkIdx: 0, threadId: `t${lineIdx}`, side: "unified", height: 50, identity: `it${lineIdx}` });
    }
  });
  rows.push({ type: "file-header", filePath: "next.md", fileIndex: 1, sourceIndex: 1, height: 30, identity: "fh2", additions: 0, deletions: 0 });
  return { file, rows };
}

const ctx = (n: number) => ({ kind: "context" as const, old_num: n, new_num: n });
const add = (n: number) => ({ kind: "add" as const, old_num: null, new_num: n });
const del = (n: number) => ({ kind: "del" as const, old_num: n, new_num: null });

describe("sideBlockSpans", () => {
  it("covers each block up to the next one, including threads and blank lines", () => {
    const { file, rows } = fixture([ctx(1), ctx(2), ctx(3), ctx(4)], [1]);
    const spans = sideBlockSpans([block(1, 2), block(4, 4)], rows, 0, file);
    expect(spans.map((s) => [s.firstRow, s.lastRow])).toEqual([[2, 5], [6, 6]]);
  });

  it("skips blocks outside the diff and keeps deleted lines before the next block", () => {
    const { file, rows } = fixture([ctx(10), del(11), add(11)]);
    const spans = sideBlockSpans([block(1, 3), block(10, 10), block(11, 12)], rows, 0, file);
    expect(spans.map((s) => [s.block.startLine, s.firstRow, s.lastRow, s.changed])).toEqual([[10, 2, 3, false], [11, 4, 4, true]]);
  });

  it("ends a span at a hunk break", () => {
    const { file, rows } = fixture([ctx(1), ctx(2)]);
    rows.splice(3, 0, { type: "hunk-header", filePath: PATH, hunkIdx: 1, header: "@@", height: 20, identity: "hh2" });
    const spans = sideBlockSpans([block(1, 1), block(5, 5)], rows, 0, file);
    expect(spans.map((s) => [s.firstRow, s.lastRow])).toEqual([[2, 2]]);
  });

  it("stops at the next file", () => {
    const { file, rows } = fixture([ctx(1)]);
    expect(sideBlockSpans([block(1, 1)], rows, 0, file)).toHaveLength(1);
    expect(sideBlockSpans([block(1, 1)], rows, rows.length - 1, file)).toEqual([]);
  });
});

describe("sideBlockPadding", () => {
  it("pads only blocks taller than their raw rows, after the last row", () => {
    const { file, rows } = fixture([ctx(1), ctx(2), ctx(3)]);
    const spans = sideBlockSpans([block(1, 2), block(3, 3)], rows, 0, file);
    const offsets = [0];
    for (const row of rows) offsets.push(offsets[offsets.length - 1] + row.height);
    const heights = new Map([[spans[0].key, 30], [spans[1].key, 75]]);
    expect([...sideBlockPadding(spans, offsets, (k) => heights.get(k))]).toEqual([[4, 55]]);
  });
});
