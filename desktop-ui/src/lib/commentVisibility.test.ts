import { describe, expect, it } from "vitest";
import {
  DEFAULT_COMMENT_VISIBILITY,
  commentThreads,
  visibleCommentThreads,
} from "./commentVisibility";
import type { ThreadSnapshot } from "./types";

function thread(
  partial: Partial<ThreadSnapshot> & Pick<ThreadSnapshot, "id" | "kind">,
): ThreadSnapshot {
  return {
    file: "a.ts",
    line: 1,
    line_end: null,
    side: "RIGHT",
    source: "github",
    synced: true,
    stale: false,
    resolved: false,
    root: {
      id: partial.id,
      author: "x",
      kind: "human",
      timestamp: "",
      body_markdown: "hi",
    },
    replies: [],
    promoted_to: null,
    ...partial,
  };
}

describe("visibleCommentThreads", () => {
  const threads = [
    thread({ id: "1", kind: "comment", resolved: true, stale: false }),
    thread({ id: "2", kind: "comment", resolved: false, stale: true }),
    thread({ id: "3", kind: "comment", resolved: false, stale: false }),
    thread({ id: "4", kind: "note", resolved: false, stale: false }),
  ];

  it("counts only comment kind", () => {
    expect(commentThreads(threads)).toHaveLength(3);
  });

  it("hides resolved and outdated by default", () => {
    const visible = visibleCommentThreads(threads, DEFAULT_COMMENT_VISIBILITY);
    expect(visible.map((t) => t.id)).toEqual(["3"]);
  });

  it("can show all when filters off", () => {
    const visible = visibleCommentThreads(threads, {
      ...DEFAULT_COMMENT_VISIBILITY,
      hideResolved: false,
      hideOutdated: false,
    });
    expect(visible.map((t) => t.id)).toEqual(["1", "2", "3"]);
  });
});
