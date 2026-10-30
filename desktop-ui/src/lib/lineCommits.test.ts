import { describe, expect, it } from "bun:test";
import { lineCommitSha, lineCommitsCurrent, type LineCommits } from "./lineCommits";

const result: LineCommits = {
  path: "src/a.ts",
  cache_key: "k1",
  unavailable: null,
  commits: ["aaa", "bbb"],
  added: [
    { start: 2, len: 1, commit: 0 },
    { start: 4, len: 3, commit: 1 },
    { start: 9, len: 1, commit: 0 },
  ],
  deleted: [{ start: 3, len: 2, commit: 0 }],
  deleted_attributed: true,
};

describe("lineCommitSha", () => {
  it("finds the commit for a line inside a span", () => {
    expect(lineCommitSha(result, "add", 2)).toBe("aaa");
    expect(lineCommitSha(result, "add", 4)).toBe("bbb");
    expect(lineCommitSha(result, "add", 6)).toBe("bbb");
    expect(lineCommitSha(result, "add", 9)).toBe("aaa");
    expect(lineCommitSha(result, "del", 4)).toBe("aaa");
  });

  it("returns null between and outside spans", () => {
    for (const line of [1, 3, 7, 8, 10]) expect(lineCommitSha(result, "add", line)).toBeNull();
    expect(lineCommitSha(result, "del", 5)).toBeNull();
    expect(lineCommitSha({ ...result, added: [] }, "add", 2)).toBeNull();
  });
});

describe("lineCommitsCurrent", () => {
  it("drops replies for another file or outdated hunks", () => {
    expect(lineCommitsCurrent(result, "src/a.ts", "k1")).toBe(true);
    expect(lineCommitsCurrent(result, "src/a.ts", "k2")).toBe(false);
    expect(lineCommitsCurrent(result, "src/b.ts", "k1")).toBe(false);
  });
});
