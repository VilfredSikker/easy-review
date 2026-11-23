import { describe, expect, it } from "bun:test";
import type { Hunk } from "$lib/protocol/generated/Hunk";
import type { ThreadSnapshot } from "$lib/pageTypes";
import { joinThreads, threadOnHunk } from "./threadJoin";

/** A hunk with `header`'s ranges and no lines: the join reads the header's span. */
function hunk(header: string): Hunk {
  return { header, old_start: 0, old_count: 0, new_start: 0, new_count: 0, lines: [] };
}

function thread(id: string, over: Partial<ThreadSnapshot> = {}): ThreadSnapshot {
  const root = { id, author: "You", kind: "you" as const, timestamp: "", body_markdown: "" };
  return { id, kind: "comment", file: "p.py", line: 0, side: "RIGHT", source: "github", stale: false, resolved: false, root, replies: [], promoted_to: null, ...over };
}

// The cases of the engine's `comments_for_hunk_or_line_range` tests, so the
// join and the App's placement agree.
describe("threadOnHunk", () => {
  it("places a thread with no line on the hunk it names", () => {
    const t = thread("t", { hunk_index: 1 });
    expect([threadOnHunk(t, hunk("@@ -1,2 +1,2 @@"), 0), threadOnHunk(t, hunk("@@ -9,2 +9,2 @@"), 1)]).toEqual([false, true]);
  });

  it("matches a line in the new range even when the hunk index it was written with is stale", () => {
    expect(threadOnHunk(thread("q", { line: 15, hunk_index: 99 }), hunk("@@ -10,10 +10,10 @@"), 0)).toBe(true);
  });

  it("matches a LEFT line in the old range of a full-file delete", () => {
    expect(threadOnHunk(thread("q", { line: 1, side: "LEFT" }), hunk("@@ -1,28 +0,0 @@"), 0)).toBe(true);
  });

  it("never re-attaches a new-side line to a later hunk's old range, delete-only hunks included", () => {
    const t = thread("c", { line: 222 });
    expect(threadOnHunk(t, hunk("@@ -140,20 +218,20 @@"), 0)).toBe(true);
    expect(threadOnHunk(t, hunk("@@ -200,50 +250,26 @@"), 1)).toBe(false);
    expect(threadOnHunk(t, hunk("@@ -200,50 +250,0 @@"), 1)).toBe(false);
  });

  it("keeps a lined thread off a hunk that does not hold its line, whatever its hunk index", () => {
    const t = thread("c", { line: 222, hunk_index: 1 });
    expect(threadOnHunk(t, hunk("@@ -170,50 +250,26 @@"), 1)).toBe(false);
  });

  it("places a LEFT line by its old-side number, never on an earlier hunk's new side", () => {
    const t = thread("c", { line: 222, old_line: 222, side: "LEFT" });
    expect(threadOnHunk(t, hunk("@@ -140,20 +218,20 @@"), 0)).toBe(false);
    expect(threadOnHunk(t, hunk("@@ -200,50 +250,26 @@"), 1)).toBe(true);
    expect(threadOnHunk(thread("m", { line: 10, old_line: 10, side: "LEFT" }), hunk("@@ -8,12 +20,8 @@"), 0)).toBe(true);
  });

  it("reads the header's span, not the lines a fold left visible", () => {
    const folded: Hunk = { ...hunk("@@ -1,300 +1,310 @@"), new_start: 1, new_count: 8 };
    expect(threadOnHunk(thread("gh", { line: 288 }), folded, 0)).toBe(true);
  });
});

describe("joinThreads", () => {
  it("puts each thread on its hunk, in the order the annotations list them, and defaults the side", () => {
    const hunks = [hunk("@@ -1,5 +1,5 @@"), hunk("@@ -20,5 +20,5 @@")];
    const joined = joinThreads(hunks, [thread("a", { line: 21 }), thread("b", { line: 2, side: "" }), thread("c", { line: 22 })]);
    expect(joined.map((h) => h.threads.map((t) => [t.id, t.side]))).toEqual([[["b", "RIGHT"]], [["a", "RIGHT"], ["c", "RIGHT"]]]);
  });
});
