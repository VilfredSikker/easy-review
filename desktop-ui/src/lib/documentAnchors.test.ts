import { describe, expect, it } from "bun:test";
import { hunkForLines, quotedBody, quoteSourceLines, threadsByBlock } from "./documentAnchors";
import { documentTextBlocks, type DocumentBlock } from "./documentPreview";
import type { FileSnapshot, LineSnapshot, ThreadSnapshot } from "./types";

const SOURCE = [
  "# Design", //                                   1
  "", //                                           2
  "The cache keeps **every** entry until the", //  3
  "process exits, which is fine for a CLI.", //    4
  "", //                                           5
  "- first item", //                               6
  "- second [link](https://x.dev) item", //        7
].join("\n");

describe("quoteSourceLines", () => {
  it("narrows a highlight to the source line holding it", () => {
    expect(quoteSourceLines(SOURCE, { start: 3, end: 4 }, "fine for a CLI")).toEqual({ start: 4, end: 4 });
  });

  it("matches through markup the rendered text drops", () => {
    expect(quoteSourceLines(SOURCE, { start: 3, end: 4 }, "keeps every entry")).toEqual({ start: 3, end: 3 });
    expect(quoteSourceLines(SOURCE, { start: 6, end: 7 }, "second link item")).toEqual({ start: 7, end: 7 });
  });

  it("spans the lines a highlight wraps across", () => {
    expect(quoteSourceLines(SOURCE, { start: 3, end: 4 }, "until the process exits")).toEqual({ start: 3, end: 4 });
  });

  it("keeps line numbers right after an emoji earlier in the range", () => {
    const source = "Ship it 🚀 today\nthen rest";
    expect(quoteSourceLines(source, { start: 1, end: 2 }, "then rest")).toEqual({ start: 2, end: 2 });
  });

  it("prefers a whole-word match for a short highlight", () => {
    const source = "this line\nwhat is it";
    expect(quoteSourceLines(source, { start: 1, end: 2 }, "is")).toEqual({ start: 2, end: 2 });
  });

  it("still matches a highlight that starts mid-word", () => {
    expect(quoteSourceLines(SOURCE, { start: 3, end: 4 }, "ntil the process")).toEqual({ start: 3, end: 4 });
  });

  it("falls back to the blocks' range when the words are not found", () => {
    expect(quoteSourceLines(SOURCE, { start: 3, end: 4 }, "nothing like it")).toEqual({ start: 3, end: 4 });
    expect(quoteSourceLines(SOURCE, { start: 3, end: 4 }, "  ")).toEqual({ start: 3, end: 4 });
  });
});

function line(new_num: number | null, kind: LineSnapshot["kind"]): LineSnapshot {
  return { old_num: null, new_num, kind, text: "" };
}

function file(hunks: LineSnapshot[][]): FileSnapshot {
  return {
    path: "README.md",
    hunks: hunks.map((lines) => ({ header: "", old_start: 0, old_count: 0, new_start: 0, new_count: 0, lines, threads: [] })),
  } as unknown as FileSnapshot;
}

describe("hunkForLines", () => {
  const doc = file([
    [line(3, "context"), line(4, "add"), line(null, "del")],
    [line(10, "add"), line(11, "add")],
  ]);

  it("finds the hunk holding every line", () => {
    expect(hunkForLines(doc, { start: 3, end: 4 })).toBe(0);
    expect(hunkForLines(doc, { start: 11, end: 11 })).toBe(1);
  });

  it("is null when any line is off the diff, even if others are in it", () => {
    expect(hunkForLines(doc, { start: 6, end: 6 })).toBeNull();
    expect(hunkForLines(doc, { start: 4, end: 5 })).toBeNull();
    expect(hunkForLines(doc, { start: 4, end: 10 })).toBeNull();
  });
});

describe("quotedBody", () => {
  it("opens the body with the highlight on one quote line", () => {
    expect(quotedBody("keeps\n every   entry", "  Why?  ")).toBe("> keeps every entry\n\nWhy?");
  });

  it("clips a long highlight", () => {
    const body = quotedBody("x".repeat(500), "ok");
    expect(body.split("\n")[0].length).toBe(202);
    expect(body.split("\n")[0].endsWith("…")).toBe(true);
  });
});

function thread(id: string, lineNum: number, extra: Partial<ThreadSnapshot> = {}): ThreadSnapshot {
  return { id, file: "README.md", line: lineNum, side: "RIGHT", resolved: false, ...extra } as ThreadSnapshot;
}

describe("threadsByBlock", () => {
  const blocks = [{ startLine: 1, endLine: 1 }, { startLine: 3, endLine: 4 }] as DocumentBlock[];

  it("puts each thread after the block holding its line", () => {
    const placed = threadsByBlock(blocks, [thread("a", 4), thread("b", 1), thread("c", 9)], "README.md");
    expect(placed.get(3)?.map((t) => t.id)).toEqual(["a"]);
    expect(placed.get(1)?.map((t) => t.id)).toEqual(["b"]);
    expect(placed.get(-1)?.map((t) => t.id)).toEqual(["c"]);
  });

  it("skips other files, the old side, resolved and file-level threads", () => {
    const placed = threadsByBlock(blocks, [
      thread("other", 3, { file: "x.md" }),
      thread("old", 3, { side: "LEFT" }),
      thread("done", 3, { resolved: true }),
      thread("file", 0),
    ], "README.md");
    expect(placed.size).toBe(0);
  });
});

describe("documentTextBlocks", () => {
  it("splits at blank lines and keeps each paragraph's source lines", () => {
    const blocks = documentTextBlocks("one\ntwo\n\n\nthree <b>\n");
    expect(blocks.map((b) => [b.startLine, b.endLine])).toEqual([[1, 2], [5, 5]]);
    expect(blocks[1].segment).toEqual({ kind: "html", html: '<pre class="document-text">three &lt;b&gt;</pre>' });
  });
});
