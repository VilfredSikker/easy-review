import { describe, expect, it } from "vitest";
import {
  DEFAULT_COMMENT_VISIBILITY,
  commentFilterCounts,
  commentThreads,
  hiddenCommentsHint,
  hiddenGithubHint,
  kindShown,
  toggleCommentFilter,
  toggleKindShown,
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
    // Inline rows read hunk.threads; the panel must read the same copy.
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

describe("unpushedLocalCommentThreads", () => {
  it("counts own unpushed comments, not GitHub threads already on the PR", () => {
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

// A `hideAll` stored by the old Comments panel, which the redesigned UI had no
// control for: every annotation stayed hidden from the diff with no way back.
const STUCK_HIDE_ALL = {
  ...DEFAULT_COMMENT_VISIBILITY,
  hideAll: true,
  hideResolved: true,
  hideOutdated: true,
};

describe("kindShown", () => {
  it("reads unticked under hideAll even when the kind itself is on", () => {
    expect(kindShown(STUCK_HIDE_ALL, "hideQuestions")).toBe(false);
    expect(kindShown(DEFAULT_COMMENT_VISIBILITY, "hideQuestions")).toBe(true);
  });

  it("reads unticked when the kind is hidden", () => {
    expect(kindShown({ ...DEFAULT_COMMENT_VISIBILITY, hideNotes: true }, "hideNotes")).toBe(false);
  });
});

describe("toggleKindShown", () => {
  it("lifts hideAll and shows the clicked kind", () => {
    const next = { ...STUCK_HIDE_ALL, ...toggleKindShown(STUCK_HIDE_ALL, "hideComments") };
    expect(next.hideAll).toBe(false);
    expect(kindShown(next, "hideComments")).toBe(true);
    // The other kinds were on underneath hideAll, so they come back too.
    expect(kindShown(next, "hideQuestions")).toBe(true);
  });

  it("shows a hidden kind under hideAll instead of hiding it further", () => {
    const vis = { ...STUCK_HIDE_ALL, hideNotes: true };
    const next = { ...vis, ...toggleKindShown(vis, "hideNotes") };
    expect(kindShown(next, "hideNotes")).toBe(true);
  });

  it("flips just the kind when hideAll is off", () => {
    expect(toggleKindShown(DEFAULT_COMMENT_VISIBILITY, "hideFindings")).toEqual({ hideFindings: true });
    expect(toggleKindShown(DEFAULT_COMMENT_VISIBILITY, "hideOutdated")).toEqual({ hideOutdated: false });
  });
});

describe("commentFilterCounts", () => {
  it("counts resolved and outdated GitHub comments, not questions or notes", () => {
    const counts = commentFilterCounts([
      thread({ id: "c1", kind: "comment", resolved: true }),
      thread({ id: "c2", kind: "comment", stale: true }),
      thread({ id: "c3", kind: "comment", resolved: true, stale: true }),
      thread({ id: "q1", kind: "question", resolved: true, stale: true }),
    ]);
    expect(counts).toEqual({ resolved: 2, outdated: 2 });
  });
});

describe("hiddenGithubHint", () => {
  it("names hideAll when that is what hides the comments", () => {
    expect(hiddenGithubHint(1, STUCK_HIDE_ALL)).toBe(
      "1 GitHub comment hidden: all annotations are hidden. Tick Comments in the display menu to show them.",
    );
  });

  it("names the resolved/outdated filters otherwise", () => {
    expect(hiddenGithubHint(2, DEFAULT_COMMENT_VISIBILITY)).toBe(
      "2 GitHub comments hidden (resolved or outdated). Show them from the display menu.",
    );
  });
});
