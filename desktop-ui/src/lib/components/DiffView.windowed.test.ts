/**
 * Per-file height estimation for DiffView virtualization.
 *
 * The full virtualization (IntersectionObserver-driven mount/unmount) is
 * DOM-dependent and not exercised here. We only verify the pure height-
 * estimation formula, which is what keeps the stub placeholder close enough
 * to the real body height that scroll position stays put when a file
 * un-mounts.
 *
 * Formula (must match DiffView.svelte::estimateHeight):
 *   sum(22 + lines.length * 21 for hunk in hunks) || 60
 *   - 21px per line (mono leading-[1.55] @ 13px)
 *   - 22px per hunk header row
 *   - 60px floor for empty/no-change files
 */
import { describe, expect, it } from "bun:test";
import type { FileSnapshot, HunkSnapshot, LineSnapshot } from "$lib/types";

function estimateHeight(file: FileSnapshot): number {
  return file.hunks.reduce((acc, h) => acc + 22 + h.lines.length * 21, 0) || 60;
}

function line(): LineSnapshot {
  return { old_num: 1, new_num: 1, kind: "context", spans: [{ text: "x", color: "" }] };
}

function hunk(lineCount: number): HunkSnapshot {
  const lines: LineSnapshot[] = [];
  for (let i = 0; i < lineCount; i++) lines.push(line());
  return {
    header: "@@ ...",
    old_start: 1,
    old_count: lineCount,
    new_start: 1,
    new_count: lineCount,
    lines,
    threads: [],
  };
}

function makeFile(hunks: HunkSnapshot[]): FileSnapshot {
  return {
    path: "x.ts",
    status: "modified",
    additions: 0,
    deletions: 0,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks,
  };
}

describe("estimateHeight", () => {
  it("returns the 60px floor for a file with no hunks", () => {
    expect(estimateHeight(makeFile([]))).toBe(60);
  });

  it("uses 22px header + 21px/line for a single hunk", () => {
    // 1 hunk × (22 + 10 lines × 21) = 22 + 210 = 232
    expect(estimateHeight(makeFile([hunk(10)]))).toBe(232);
  });

  it("sums across multiple hunks", () => {
    // (22 + 3*21) + (22 + 5*21) + (22 + 1*21) = 85 + 127 + 43 = 255
    expect(estimateHeight(makeFile([hunk(3), hunk(5), hunk(1)]))).toBe(255);
  });

  it("handles a single empty hunk (header only)", () => {
    expect(estimateHeight(makeFile([hunk(0)]))).toBe(22);
  });

  it("scales linearly — doubling lines roughly doubles height", () => {
    const small = estimateHeight(makeFile([hunk(50)]));
    const big = estimateHeight(makeFile([hunk(100)]));
    // small = 22 + 1050 = 1072; big = 22 + 2100 = 2122 — within ~5% of 2x.
    expect(big).toBeGreaterThan(small * 1.9);
    expect(big).toBeLessThan(small * 2.1);
  });
});
