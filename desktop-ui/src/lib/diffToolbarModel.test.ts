import { describe, expect, it } from "bun:test";
import {
  commitNote,
  DIFF_EMPTY_TEXT,
  diffEmptyState,
  nextVisibleFile,
  reviewedCount,
  visibleDiffFiles,
} from "./diffToolbarModel";

const files = [
  { path: "a.ts", reviewed: true },
  { path: "b.ts", reviewed: false },
  { path: "c.ts", reviewed: true },
];

const commits = [
  { sha: "1111111aaaa", title: "First" },
  { sha: "2222222bbbb", title: "Second" },
];

describe("visibleDiffFiles", () => {
  it("drops reviewed files when hiding them, keeping order", () => {
    expect(visibleDiffFiles(files, true, "branch").map((f) => f.path)).toEqual(["b.ts"]);
  });

  it("returns the list untouched when not hiding", () => {
    expect(visibleDiffFiles(files, false, "branch")).toBe(files);
  });

  it("leaves the Tour's files alone, since its pillars are laid out from all of them", () => {
    expect(visibleDiffFiles(files, true, "tour")).toBe(files);
  });
});

describe("nextVisibleFile", () => {
  const ordered = [
    { path: "a.ts" },
    { path: "b.ts" },
    { path: "c.ts" },
    { path: "d.ts" },
  ];
  const pick = (path: string, visible: string[]) =>
    nextVisibleFile(ordered, ordered.filter((f) => visible.includes(f.path)), path)?.path ?? null;

  it("moves to the next drawn file when the collapsed one is still drawn", () => {
    expect(pick("b.ts", ["a.ts", "b.ts", "c.ts", "d.ts"])).toBe("c.ts");
  });

  it("stays on the last drawn file when nothing follows it", () => {
    expect(pick("d.ts", ["a.ts", "d.ts"])).toBe("d.ts");
  });

  it("skips hidden files to reach the next drawn one after a hidden path", () => {
    expect(pick("b.ts", ["a.ts", "d.ts"])).toBe("d.ts");
  });

  it("falls back to the nearest drawn file before a hidden path", () => {
    expect(pick("c.ts", ["a.ts", "b.ts"])).toBe("b.ts");
  });

  it("returns null when nothing is drawn, or the path is unknown", () => {
    expect(pick("b.ts", [])).toBeNull();
    expect(pick("z.ts", ["a.ts"])).toBeNull();
  });
});

describe("diffEmptyState", () => {
  it("blames Hide reviewed when the view has files", () => {
    expect(diffEmptyState("branch", 3)).toBe("all-reviewed");
    expect(DIFF_EMPTY_TEXT["all-reviewed"]).toContain("Hide reviewed");
  });

  it("points an empty Staged scope at staging", () => {
    expect(diffEmptyState("staged", 0)).toBe("nothing-staged");
    expect(DIFF_EMPTY_TEXT["nothing-staged"]).toContain("git add");
  });

  it("falls back to no changes elsewhere", () => {
    expect(diffEmptyState("unstaged", 0)).toBe("no-changes");
  });
});

describe("commitNote", () => {
  it("names the selected commit with its rail colour index and title", () => {
    expect(commitNote(commits, "2222222bbbb", false)).toEqual({
      kind: "commit",
      chip: { sha: "2222222", colorIndex: 1 },
      title: "Second",
    });
  });

  it("prefers the single commit over the Mark lines legend", () => {
    expect(commitNote(commits, "1111111aaaa", true)?.kind).toBe("commit");
  });

  it("keeps a commit missing from the list, uncoloured and untitled", () => {
    expect(commitNote(commits, "9999999zzzz", false)).toEqual({
      kind: "commit",
      chip: { sha: "9999999", colorIndex: null },
      title: "",
    });
  });

  it("shows the legend while lines are marked", () => {
    expect(commitNote(commits, null, true)).toEqual({
      kind: "legend",
      chips: [
        { sha: "1111111", colorIndex: 0 },
        { sha: "2222222", colorIndex: 1 },
      ],
    });
  });

  it("says nothing otherwise", () => {
    expect(commitNote(commits, null, false)).toBeNull();
    expect(commitNote([], null, true)).toBeNull();
  });
});

describe("reviewedCount", () => {
  it("counts reviewed files", () => {
    expect(reviewedCount(files)).toBe(2);
  });
});
