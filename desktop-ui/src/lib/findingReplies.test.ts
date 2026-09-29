import { describe, expect, test } from "bun:test";
import { mergeFindingReplies } from "./findingReplies";
import type { FindingResponseSnapshot, FlatFinding, ThreadMessage, ThreadSnapshot } from "./types";

function response(over: Partial<FindingResponseSnapshot>): FindingResponseSnapshot {
  return {
    id: "r1",
    author: "AI",
    kind: "ai",
    timestamp: "2026-01-01T00:00:00Z",
    body_markdown: "body",
    origin: "finding_response",
    editable: false,
    deletable: true,
    ...over,
  };
}

function finding(responses: FindingResponseSnapshot[]): FlatFinding {
  return { id: "f1", responses } as FlatFinding;
}

function thread(replies: Partial<ThreadMessage>[]): ThreadSnapshot {
  return { id: "t1", replies: replies as ThreadMessage[] } as ThreadSnapshot;
}

describe("mergeFindingReplies", () => {
  test("merges both sources and sorts by timestamp", () => {
    const merged = mergeFindingReplies(
      finding([response({ id: "a", timestamp: "2026-01-03T00:00:00Z" })]),
      thread([{ id: "b", author: "me", kind: "you", timestamp: "2026-01-02T00:00:00Z", body_markdown: "hi" }]),
    );
    expect(merged.map((r) => r.id)).toEqual(["b", "a"]);
    expect(merged[0]).toMatchObject({ origin: "thread_reply", editable: true, deletable: true });
    expect(merged[1]).toMatchObject({ origin: "finding_response", editable: false });
  });

  test("an arbiter response is still a finding_response", () => {
    const [merged] = mergeFindingReplies(finding([response({ origin: "arbiter" })]), null);
    expect(merged.origin).toBe("finding_response");
  });

  test("keeps the first copy of a duplicate and keys stay unique", () => {
    const merged = mergeFindingReplies(
      finding([response({ id: "a", author: "first" }), response({ id: "a", author: "second" })]),
      null,
    );
    expect(merged).toHaveLength(1);
    expect(merged[0].author).toBe("first");
  });

  test("an empty id falls back to the timestamp in the key", () => {
    const merged = mergeFindingReplies(
      finding([
        response({ id: "", timestamp: "2026-01-01T00:00:00Z" }),
        response({ id: "", timestamp: "2026-01-02T00:00:00Z" }),
      ]),
      null,
    );
    expect(merged).toHaveLength(2);
    expect(new Set(merged.map((r) => r.key)).size).toBe(2);
  });
});
