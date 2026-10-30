import { describe, expect, it } from "bun:test";
import { renameSpans, renamedFromLabel, similarityLabel, splitSpan } from "./renameDiff";

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

describe("similarityLabel", () => {
  it("is empty when unreported", () => {
    expect(similarityLabel(null)).toBe("");
    expect(similarityLabel(100)).toBe("similarity 100%");
  });
});
