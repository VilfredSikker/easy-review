import { describe, expect, it } from "bun:test";
import type { ThreadMessage, ThreadSnapshot } from "./types";
import { entryWhere, reviewerEntries, reviewerStateLabel, reviewerStateTone, sameAuthor } from "./reviewerComments";

function msg(id: string, author: string, kind: ThreadMessage["kind"] = "human", body = `${id} body`): ThreadMessage {
  return { id, author, kind, timestamp: "2026-10-30T10:00:00Z", body_markdown: body };
}

function thread(id: string, at: { file: string; line: number }, root: ThreadMessage, replies: ThreadMessage[] = [], kind: ThreadSnapshot["kind"] = "comment"): ThreadSnapshot {
  return { id, kind, ...at, source: "github", synced: true, stale: false, resolved: false, root, replies, promoted_to: null };
}

describe("reviewerEntries", () => {
  const threads = [
    thread("t2", { file: "src/b.ts", line: 4 }, msg("r2", "martin")),
    thread("t1", { file: "db/schema.sql", line: 12 }, msg("r1", "sune"), [msg("x1", "martin"), msg("x2", "martin", "ai"), msg("x3", "sune")]),
    thread("q1", { file: "src/a.ts", line: 1 }, msg("q", "martin"), [], "question"),
  ];

  it("lists reviews, then PR comments, then inline comments and replies by file", () => {
    const entries = reviewerEntries("martin", {
      reviews: [
        { author: "martin", state: "COMMENTED", body: "Looks close", submitted_at: "2026-10-30T09:00:00Z" },
        { author: "martin", state: "APPROVED", body: "", submitted_at: "2026-10-30T11:00:00Z" },
        { author: "sune", state: "COMMENTED", body: "mine", submitted_at: "2026-10-30T11:00:00Z" },
      ],
      comments: [{ author: "martin", body: " Ship it ", created_at: "2026-10-30T12:00:00Z", url: "u1" }],
      threads,
    });
    expect(entries.map((e) => [e.kind, e.text, entryWhere(e)])).toEqual([
      ["review", "Approved", "review"],
      ["review", "Looks close", "review"],
      ["pr", "Ship it", "PR"],
      ["reply", "x1 body", "schema.sql:12"],
      ["comment", "r2 body", "b.ts:4"],
    ]);
    expect(entries[3].threadId).toBe("t1");
  });

  it("leaves out AI replies, questions and notes", () => {
    const entries = reviewerEntries("martin", { threads });
    expect(entries.map((e) => e.key)).toEqual(["r:t1:x1", "c:t2"]);
  });

  it("is empty for someone who left nothing", () => {
    expect(reviewerEntries("nobody", { threads })).toEqual([]);
  });
});

describe("sameAuthor", () => {
  it("treats Copilot's three logins as one, ignoring case and the bot suffix", () => {
    expect(sameAuthor("Copilot", "copilot-pull-request-reviewer[bot]")).toBe(true);
    expect(sameAuthor("Martin", "martin")).toBe(true);
    expect(sameAuthor("martin", "martina")).toBe(false);
  });
});

describe("reviewer state", () => {
  it("labels the verdict, or the request before one", () => {
    expect(reviewerStateLabel({ state: "APPROVED", requested: false })).toBe("approved");
    expect(reviewerStateLabel({ state: "CHANGES_REQUESTED", requested: false })).toBe("changes requested");
    expect(reviewerStateLabel({ state: null, requested: true })).toBe("requested");
    expect(reviewerStateLabel({ state: null, requested: false })).toBe("");
  });

  it("colours approval and change requests only", () => {
    expect(reviewerStateTone("APPROVED")).toBe("ok");
    expect(reviewerStateTone("CHANGES_REQUESTED")).toBe("block");
    expect(reviewerStateTone("COMMENTED")).toBe("neutral");
  });

  it("shows a line range", () => {
    expect(entryWhere({ key: "", kind: "comment", text: "", at: "", file: "a/b.ts", line: 3, lineEnd: 7, threadId: "t" })).toBe("b.ts:3–7");
  });
});
