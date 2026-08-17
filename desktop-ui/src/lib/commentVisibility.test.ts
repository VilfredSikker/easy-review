import { describe, expect, it } from "vitest";
import {
  DEFAULT_COMMENT_VISIBILITY,
  commentThreads,
  hiddenCommentsHint,
  toggleCommentFilter,
  unpushedLocalCommentThreads,
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

  it("push count is own unpushed comments, not GitHub threads already on the PR", () => {
    // Symptom: Push as review showed "7 comments" when the user had 3 local
    // drafts and 4 existing GitHub comments.
    const mixed = [
      thread({ id: "l1", kind: "comment", source: "local", synced: false }),
      thread({ id: "l2", kind: "comment", source: "local", synced: false }),
      thread({ id: "l3", kind: "comment", source: "local", synced: false }),
      thread({ id: "g1", kind: "comment", source: "github", synced: true }),
      thread({ id: "g2", kind: "comment", source: "github", synced: true }),
      thread({ id: "g3", kind: "comment", source: "github", synced: true }),
      thread({ id: "g4", kind: "comment", source: "github", synced: true }),
    ];
    expect(commentThreads(mixed)).toHaveLength(7);
    expect(unpushedLocalCommentThreads(mixed).map((t) => t.id)).toEqual([
      "l1",
      "l2",
      "l3",
    ]);
  });

  it("does not count local comments that are already synced", () => {
    const mixed = [
      thread({ id: "draft", kind: "comment", source: "local", synced: false }),
      thread({ id: "pushed", kind: "comment", source: "local", synced: true }),
      thread({ id: "gh", kind: "comment", source: "github", synced: true }),
    ];
    expect(unpushedLocalCommentThreads(mixed).map((t) => t.id)).toEqual(["draft"]);
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

  it("hideAll empties the list even when resolved/outdated filters are off", () => {
    const visible = visibleCommentThreads(threads, {
      ...DEFAULT_COMMENT_VISIBILITY,
      hideAll: true,
      hideResolved: false,
      hideOutdated: false,
    });
    expect(visible).toHaveLength(0);
  });

  it("default filters still show active GitHub threads (11 open + 4 resolved/outdated)", () => {
    const prThreads = [
      ...Array.from({ length: 4 }, (_, i) =>
        thread({ id: `r${i}`, kind: "comment", resolved: true, stale: true }),
      ),
      ...Array.from({ length: 11 }, (_, i) =>
        thread({ id: `a${i}`, kind: "comment", resolved: false, stale: false }),
      ),
    ];
    const visible = visibleCommentThreads(prThreads, DEFAULT_COMMENT_VISIBILITY);
    expect(visible).toHaveLength(11);
    expect(visible.every((t) => !t.resolved && !t.stale)).toBe(true);
  });

  it("prefers diff hunk threads over ai.threads so the panel matches inline", () => {
    // Symptom: comments visible while scrolling the diff, empty Comments panel.
    // Inline rows read hunk.threads; the panel used to read only ai.threads.
    const aiStale = [
      thread({ id: "gh-1", kind: "comment", stale: true, resolved: false }),
      thread({ id: "gh-2", kind: "comment", stale: true, resolved: false }),
    ];
    const files = [
      {
        hunks: [
          {
            threads: [
              thread({ id: "gh-1", kind: "comment", stale: false, resolved: false }),
              thread({ id: "gh-2", kind: "comment", stale: false, resolved: false }),
            ],
          },
        ],
      },
    ];
    const visible = visibleCommentThreads(aiStale, DEFAULT_COMMENT_VISIBILITY, files);
    expect(visible.map((t) => t.id)).toEqual(["gh-1", "gh-2"]);
  });
});

describe("toggleCommentFilter", () => {
  it("clears hideAll when toggling hide outdated so the click can reveal comments", () => {
    const next = toggleCommentFilter(
      { ...DEFAULT_COMMENT_VISIBILITY, hideAll: true, hideOutdated: true },
      "hideOutdated",
    );
    expect(next.hideAll).toBe(false);
    expect(next.hideOutdated).toBe(false);
  });

  it("clears hideAll when toggling hide resolved", () => {
    const next = toggleCommentFilter(
      { ...DEFAULT_COMMENT_VISIBILITY, hideAll: true, hideResolved: true },
      "hideResolved",
    );
    expect(next.hideAll).toBe(false);
    expect(next.hideResolved).toBe(false);
  });

  it("toggles hideAll on its own without touching the other filters", () => {
    const on = toggleCommentFilter(DEFAULT_COMMENT_VISIBILITY, "hideAll");
    expect(on.hideAll).toBe(true);
    expect(on.hideResolved).toBe(true);
    expect(on.hideOutdated).toBe(true);
    const off = toggleCommentFilter(on, "hideAll");
    expect(off.hideAll).toBe(false);
  });
});

describe("hiddenCommentsHint", () => {
  it("tells the user to turn off Hide all when that filter emptied the list", () => {
    expect(
      hiddenCommentsHint(15, { ...DEFAULT_COMMENT_VISIBILITY, hideAll: true }),
    ).toBe("15 comments hidden. Turn off Hide all above to show them.");
  });

  it("mentions resolved/outdated when Hide all is off", () => {
    expect(hiddenCommentsHint(15, DEFAULT_COMMENT_VISIBILITY)).toBe(
      "15 comments hidden (resolved/outdated). Turn off Hide resolved or Hide outdated above to show them.",
    );
  });
});
