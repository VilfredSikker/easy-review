import { describe, expect, test } from "bun:test";
import {
  clampPopoverTop,
  fileMapGroups,
  rulerFileCount,
  tickGap,
  tickState,
  tourGroups,
} from "./rulerGeometry";
import type { FileSnapshot, PillarSnapshot } from "./types";

function file(path: string, over: Partial<FileSnapshot> = {}): FileSnapshot {
  return {
    path,
    status: "modified",
    additions: 1,
    deletions: 0,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [],
    source_index: 0,
    cache_key: path,
    ...over,
  };
}

function pillar(id: string, title: string, paths: string[]): PillarSnapshot {
  return {
    id,
    title,
    descriptionMarkdown: "",
    importance: 1,
    foundation: false,
    files: paths.map((path) => ({ path })) as PillarSnapshot["files"],
    reviewedCount: 0,
    totalCount: paths.length,
  };
}

describe("fileMapGroups", () => {
  test("one untitled group, filter applied, current flagged", () => {
    const files = [file("a.ts"), file("b.ts"), file("c.md")];
    const [g] = fileMapGroups(files, "b.ts", (f) => f.path.endsWith(".ts"));
    expect(g.title).toBeNull();
    expect(g.items.map((i) => i.path)).toEqual(["a.ts", "b.ts"]);
    expect(g.items[1].current).toBe(true);
  });
});

describe("tourGroups", () => {
  test("numbers pillars and trails the unclaimed files", () => {
    const files = [file("a.ts"), file("b.ts"), file("c.ts")];
    const groups = tourGroups([pillar("p1", "Core", ["a.ts"]), pillar("p2", "UI", ["b.ts"])], files, null);
    expect(groups.map((g) => g.title)).toEqual(["1. Core", "2. UI", "Other files"]);
    expect(rulerFileCount(groups)).toBe(3);
  });

  test("a group emptied by the filter disappears", () => {
    const files = [file("a.ts"), file("b.md")];
    const groups = tourGroups(
      [pillar("p1", "Core", ["a.ts"]), pillar("p2", "Docs", ["b.md"])],
      files,
      null,
      (f) => f.path.endsWith(".ts"),
    );
    expect(groups.map((g) => g.title)).toEqual(["1. Core"]);
  });
});

describe("tickState", () => {
  test("precedence: current, annotated, reviewed, plain", () => {
    const items = fileMapGroups(
      [file("cur"), file("note", { comment_count: 1, reviewed: true }), file("done", { reviewed: true }), file("x")],
      "cur",
    )[0].items;
    expect(items.map(tickState)).toEqual(["cur", "has", "rev", "plain"]);
  });
});

describe("tickGap", () => {
  test("roomy edge keeps the full gap", () => {
    expect(tickGap(5, 1, 400)).toBe(4);
  });
  test("crowded edge compresses to zero, never negative", () => {
    expect(tickGap(500, 3, 300)).toBe(0);
  });
  test("middling edge shrinks proportionally", () => {
    expect(tickGap(50, 1, 200)).toBe(2);
  });
});

describe("clampPopoverTop", () => {
  test("stays inside the container", () => {
    expect(clampPopoverTop(-20, 300, 100)).toBe(4);
    expect(clampPopoverTop(290, 300, 100)).toBe(196);
    expect(clampPopoverTop(50, 300, 100)).toBe(50);
  });
});
