import { describe, expect, it } from "bun:test";
import { findComposerAnchorRow, foldRowExtras, type AnchorScanRow } from "$lib/composerPlacement";

const FILE = "src/Media.svelte";

/** Row list mirroring a flat diff: file header, hunk header, content rows, an
 *  inline comment thread between two of them, then more content. */
const rows: AnchorScanRow[] = [
  { filePath: FILE }, // 0 file-header
  { filePath: FILE }, // 1 hunk-header
  { filePath: FILE }, // 2 line 36
  { filePath: FILE }, // 3 line 37
  { filePath: FILE }, // 4 line 38
  { filePath: FILE }, // 5 inline thread on line 38
  { filePath: FILE }, // 6 line 39
  { filePath: FILE }, // 7 line 40
  { filePath: "src/Other.ts" }, // 8 next file
];

const lines = [null, null, 36, 37, 38, null, 39, 40, 36];
const lineAt = (idx: number) => lines[idx] ?? null;

describe("findComposerAnchorRow", () => {
  it("anchors to the row of the last selected line", () => {
    expect(findComposerAnchorRow({ rows, startRow: 0, filePath: FILE, lastLine: 39, lineAt })).toBe(6);
  });

  it("skips annotation rows that carry no line of their own", () => {
    // Row 5 (the comment thread) must not be mistaken for line 39's row, and the
    // composer must land after line 39 — not on the thread above it.
    const anchor = findComposerAnchorRow({ rows, startRow: 0, filePath: FILE, lastLine: 39, lineAt });
    expect(anchor).toBeGreaterThan(5);
  });

  it("anchors to the first row when a single line is selected", () => {
    expect(findComposerAnchorRow({ rows, startRow: 0, filePath: FILE, lastLine: 36, lineAt })).toBe(2);
  });

  it("never leaves the anchor file", () => {
    // 36 also exists in src/Other.ts (row 8) — same file only.
    expect(findComposerAnchorRow({ rows, startRow: 0, filePath: FILE, lastLine: 36, lineAt })).toBe(2);
    // A line that only exists past the file boundary is not found at all.
    expect(findComposerAnchorRow({ rows, startRow: 0, filePath: FILE, lastLine: 99, lineAt })).toBeNull();
  });

  it("returns null when the line is not rendered or the file is gone", () => {
    expect(
      findComposerAnchorRow({ rows, startRow: 0, filePath: "src/Missing.ts", lastLine: 39, lineAt }),
    ).toBeNull();
    expect(findComposerAnchorRow({ rows, startRow: 0, filePath: FILE, lastLine: 12, lineAt })).toBeNull();
  });

  it("honours the start row so a later file's scan does not rescan earlier rows", () => {
    const start = 6;
    expect(findComposerAnchorRow({ rows, startRow: start, filePath: FILE, lastLine: 40, lineAt })).toBe(7);
    // Line 36 sits before `start`, so it is out of this scan's range.
    expect(findComposerAnchorRow({ rows, startRow: start, filePath: FILE, lastLine: 36, lineAt })).toBeNull();
  });
});

describe("foldRowExtras", () => {
  const base = [0, 10, 20, 30];
  const noExtras = () => 0;

  it("returns base offsets when nothing is added", () => {
    expect(foldRowExtras(base, noExtras)).toEqual([0, 10, 20, 30]);
  });

  it("keeps the anchor row's top and pushes every row after it down", () => {
    // A 50px composer band attached to row 1: row 1 keeps its top (10), while
    // the rows below move from 20/30 to 70/80.
    const offsets = foldRowExtras(base, (i) => (i === 1 ? 50 : 0));
    expect(offsets).toEqual([0, 10, 70, 80]);
    expect(offsets[1]).toBe(base[1]);
    expect(offsets[2] - base[2]).toBe(50);
    expect(offsets[3] - base[3]).toBe(50);
    expect(offsets[3]).toBe(base[3] + 50);
  });

  it("stacks extras per row and reports the total", () => {
    const offsets = foldRowExtras(base, (i) => (i === 0 ? 4 : i === 2 ? 6 : 0));
    expect(offsets).toEqual([0, 14, 24, 40]);
    expect(offsets[3]).toBe(30 + 4 + 6);
  });

  it("takes the row count from the offsets, so a short array cannot collapse rows", () => {
    expect(foldRowExtras([0, 10], () => 0)).toEqual([0, 10]);
    expect(foldRowExtras([], () => 0)).toEqual([0]);
  });
});
