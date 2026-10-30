import { describe, expect, it } from "bun:test";
import { renameHeader, renameSpans, renamedFromLabel, similarityLabel, splitSpan } from "./renameDiff";

describe("renameSpans", () => {
  it("isolates the differing middle", () => {
    const s = renameSpans("2026_a_x.sql", "2026_b_x.sql");
    expect(splitSpan("2026_a_x.sql", s.old)[1]).toBe("a");
    expect(splitSpan("2026_b_x.sql", s.new)[1]).toBe("b");
  });
  it("does not overlap prefix and suffix on pure insertion", () => {
    const s = renameSpans("ab", "aab");
    expect(s.old[0]).toBeLessThanOrEqual(s.old[1]);
    expect(splitSpan("aab", s.new)[1]).toBe("a");
  });
});

describe("renamedFromLabel", () => {
  it("shortens same-directory renames", () => {
    expect(renamedFromLabel("a/b/old.ts", "a/b/new.ts")).toBe("old.ts");
  });
  it("keeps the full path across directories", () => {
    expect(renamedFromLabel("a/old.ts", "b/new.ts")).toBe("a/old.ts");
  });
});

describe("renameHeader", () => {
  it("compares names only within one directory", () => {
    const h = renameHeader("src/lib/old.ts", "src/lib/new.ts");
    expect(h.crossDir).toBe(false);
    expect(h.old).toEqual(["", "old", ".ts"]);
    expect(h.new).toEqual(["", "new", ".ts"]);
  });
  it("marks the directory when a file moves under the same name", () => {
    const h = renameHeader("a/x.ts", "b/x.ts");
    expect(h.crossDir).toBe(true);
    expect(h.old).toEqual(["", "a", "/x.ts"]);
    expect(h.new).toEqual(["", "b", "/x.ts"]);
  });
  it("keeps the old directory when a move also renames", () => {
    const h = renameHeader("a/old.ts", "b/new.ts");
    expect(h.old.join("")).toBe("a/old.ts");
    expect(h.new.join("")).toBe("b/new.ts");
    expect(h.old[1]).toBe("a/old");
    expect(h.new[1]).toBe("b/new");
  });
  it("treats a move out of the root as cross-directory", () => {
    const h = renameHeader("x.ts", "lib/x.ts");
    expect(h.crossDir).toBe(true);
    expect(h.old).toEqual(["", "", "x.ts"]);
    expect(h.new).toEqual(["", "lib/", "x.ts"]);
  });
});

describe("similarityLabel", () => {
  it("is empty when unreported", () => {
    expect(similarityLabel(null)).toBe("");
    expect(similarityLabel(100)).toBe("similarity 100%");
  });
});
