import { describe, expect, test } from "bun:test";
import { isDocumentAnchored, ownReplyCount } from "./threadAnchor";
import type { AppSnapshot, FileSnapshot, HunkSnapshot, ThreadSnapshot } from "./types";

function thread(overrides: Partial<ThreadSnapshot> = {}): ThreadSnapshot {
  return {
    id: "q-1",
    kind: "question",
    file: "README.md",
    line: 40,
    source: "local",
    synced: false,
    stale: false,
    resolved: false,
    root: { id: "q-1", author: "you", kind: "you", timestamp: "", body_markdown: "why?" },
    replies: [],
    promoted_to: null,
    ...overrides,
  };
}

function hunk(threads: ThreadSnapshot[] = []): HunkSnapshot {
  return { header: "@@ -1,3 +1,3 @@", old_start: 1, old_count: 3, new_start: 1, new_count: 3, lines: [], threads };
}

function file(overrides: Partial<FileSnapshot> = {}): FileSnapshot {
  return {
    path: "README.md",
    status: "modified",
    additions: 1,
    deletions: 1,
    reviewed: false,
    compacted: false,
    risk: null,
    finding_count: 0,
    comment_count: 0,
    question_count: 0,
    hunks: [hunk()],
    source_index: 0,
    cache_key: "k",
    ...overrides,
  };
}

describe("isDocumentAnchored", () => {
  test("a question no hunk holds is a document anchor", () => {
    expect(isDocumentAnchored(thread(), [file()])).toBe(true);
    expect(isDocumentAnchored(thread({ kind: "note" }), [file()])).toBe(true);
  });

  test("a question its hunk holds is not", () => {
    const t = thread();
    expect(isDocumentAnchored(t, [file({ hunks: [hunk([t])] })])).toBe(false);
  });

  test("comments and file-level threads never are", () => {
    expect(isDocumentAnchored(thread({ kind: "comment" }), [file()])).toBe(false);
    expect(isDocumentAnchored(thread({ line: 0 }), [file()])).toBe(false);
  });

  test("unknown when the file's hunks are not in hand", () => {
    expect(isDocumentAnchored(thread(), undefined)).toBe(false);
    expect(isDocumentAnchored(thread(), [])).toBe(false);
    expect(isDocumentAnchored(thread(), [file({ is_lazy_stub: true, hunks: [] })])).toBe(false);
    expect(isDocumentAnchored(thread(), [file({ compacted: true, hunks: [] })])).toBe(false);
    expect(isDocumentAnchored(thread(), [file({ hunks_omitted: true, hunks: [] })])).toBe(false);
  });
});

describe("ownReplyCount", () => {
  const reply = (kind: "you" | "ai", body: string) => ({ id: `r-${body}`, author: kind, kind, timestamp: "", body_markdown: body });
  const snap = (t: ThreadSnapshot, inHunk = false) =>
    ({ ai: { threads: inHunk ? [] : [t] }, files: [file({ hunks: [hunk(inHunk ? [t] : [])] })] }) as unknown as AppSnapshot;

  test("counts the reviewer's replies with that text", () => {
    const t = thread({ replies: [reply("you", "ok"), reply("ai", "ok"), reply("you", "other"), reply("you", " ok ")] });
    expect(ownReplyCount(snap(t), "q-1", "ok")).toBe(2);
  });

  test("finds a thread held only by a hunk", () => {
    expect(ownReplyCount(snap(thread({ replies: [reply("you", "ok")] }), true), "q-1", "ok")).toBe(1);
  });

  test("zero without a snapshot or thread", () => {
    expect(ownReplyCount(null, "q-1", "ok")).toBe(0);
    expect(ownReplyCount(snap(thread()), "missing", "ok")).toBe(0);
  });
});
