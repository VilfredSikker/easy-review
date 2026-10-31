import { describe, expect, it } from "bun:test";
import type { AppSnapshot, TabSummary } from "../types";
import {
  ReviewPageMemory,
  currentReviewPage,
  reviewLayers,
  reviewPageAvailable,
  reviewPageTabKey,
  setModeArgsFor,
} from "./reviewPageModel";

type ModeSnap = Parameters<typeof setModeArgsFor>[1];

function snap(over: Partial<ModeSnap> = {}): ModeSnap {
  return {
    mode: "branch",
    tour: undefined,
    features: undefined,
    detected_pr_number: null,
    github: null,
    pr: null,
    ...over,
  } as ModeSnap;
}

function tour(scope: "pr" | "branch"): AppSnapshot["tour"] {
  return { available: true, fresh: true, scope, title: "", overviewMarkdown: "", pillars: [] };
}

function tab(over: Partial<TabSummary>): TabSummary {
  return {
    idx: 0,
    label: "",
    kind: "local_branch",
    branch: "feat",
    pr_number: null,
    remote: null,
    repo_root: "/repo",
    is_active: false,
    change_token: "",
    ...over,
  };
}

describe("currentReviewPage", () => {
  it("reads Overview from the main view and Tour/Diff from the engine mode", () => {
    expect(currentReviewPage("overview", "tour")).toBe("overview");
    expect(currentReviewPage("diff", "tour")).toBe("tour");
    expect(currentReviewPage("diff", "history")).toBe("diff");
    expect(currentReviewPage("diff", undefined)).toBe("diff");
  });

  it("is null while another main view replaces the review", () => {
    expect(currentReviewPage("settings", "branch")).toBeNull();
    expect(currentReviewPage("inbox", "tour")).toBeNull();
    expect(currentReviewPage("export-review", "branch")).toBeNull();
  });
});

describe("setModeArgsFor", () => {
  it("enters the tour once", () => {
    expect(setModeArgsFor("tour", snap({ mode: "branch" }))).toEqual({ mode: "tour" });
    expect(setModeArgsFor("tour", snap({ mode: "tour" }))).toBeNull();
  });

  it("leaves a PR tour for the PR diff, carrying the PR number", () => {
    expect(
      setModeArgsFor("diff", snap({ mode: "tour", tour: tour("pr"), detected_pr_number: 42 })),
    ).toEqual({ mode: "pr_diff", prNumber: 42 });
  });

  it("leaves a branch tour for the branch diff", () => {
    expect(setModeArgsFor("diff", snap({ mode: "tour", tour: tour("branch") }))).toEqual({ mode: "branch" });
  });

  it("does not touch a diff mode that is already showing a diff", () => {
    expect(setModeArgsFor("diff", snap({ mode: "staged" }))).toBeNull();
  });

  it("never changes the mode for the Overview", () => {
    expect(setModeArgsFor("overview", snap({ mode: "tour", tour: tour("pr") }))).toBeNull();
  });
});

describe("reviewPageAvailable", () => {
  it("hides Tour only when the view is disabled", () => {
    const off = snap({ features: { viewTour: false } as ModeSnap["features"] });
    expect(reviewPageAvailable("tour", off)).toBe(false);
    expect(reviewPageAvailable("diff", off)).toBe(true);
    expect(reviewPageAvailable("tour", snap())).toBe(true);
    expect(reviewPageAvailable("tour", null)).toBe(true);
  });

  it("hides Overview until a page that renders it has registered", () => {
    expect(reviewPageAvailable("overview", snap())).toBe(false);
    expect(reviewPageAvailable("overview", snap(), true)).toBe(true);
  });
});

describe("reviewPageTabKey", () => {
  it("ignores idx so reordering keeps the key", () => {
    const a = reviewPageTabKey({ tabs: [tab({ idx: 0, is_active: true })], active_tab: 0 });
    const b = reviewPageTabKey({ tabs: [tab({ idx: 0 }), tab({ idx: 1, is_active: true })], active_tab: 1 });
    expect(a).toBe(b);
  });

  it("keys a PR tab by its number", () => {
    expect(
      reviewPageTabKey({ tabs: [tab({ kind: "remote_pr", pr_number: 7, is_active: true })], active_tab: 0 }),
    ).toBe("/repo|remote_pr|7");
  });

  it("is null without an active tab", () => {
    expect(reviewPageTabKey(null)).toBeNull();
    expect(reviewPageTabKey({ tabs: [], active_tab: 0 })).toBeNull();
  });
});

describe("ReviewPageMemory", () => {
  it("opens a tab on its first visit on the Overview", () => {
    const m = new ReviewPageMemory();
    expect(m.follow("a", "diff")).toBe("overview");
    expect(m.follow("b", "diff")).toBe("overview");
  });

  it("keeps Diff for a first visit while the Overview is unavailable, then defaults later", () => {
    const m = new ReviewPageMemory();
    expect(m.follow("a", "diff", false)).toBeNull();
    expect(m.follow("b", "diff", false)).toBeNull();
    expect(m.follow("c", "diff", true)).toBe("overview");
  });

  it("restores the page a visited tab was left on", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "overview");
    // Reader moves to Diff on a, then opens b and leaves it on Overview.
    expect(m.follow("b", "diff")).toBe("overview");
    expect(m.follow("a", "overview")).toBe("diff");
    expect(m.follow("b", "diff")).toBe("overview");
  });

  it("does not bring the Overview back once a tab is left on Tour or Diff", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "overview");
    expect(m.follow("b", "overview")).toBeNull();
    // Reader switches b to Diff, then goes back to a and returns to b.
    expect(m.follow("a", "diff")).toBe("overview");
    expect(m.follow("b", "overview")).toBe("diff");
  });

  it("leaves a non-review main view open across tab switches", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "overview");
    expect(m.follow("b", "settings")).toBeNull();
    expect(m.follow("a", "inbox")).toBeNull();
  });

  it("does nothing while the tab stays the same", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "overview");
    expect(m.follow("a", "diff")).toBeNull();
  });
});

describe("reviewLayers", () => {
  it("keeps the diff mounted under the Overview", () => {
    expect(reviewLayers("overview", false)).toEqual({ diff: "hidden", overview: "shown" });
    expect(reviewLayers("overview", true)).toEqual({ diff: "hidden", overview: "shown" });
  });

  it("mounts the Overview behind the diff only once it has been visited", () => {
    expect(reviewLayers("diff", false)).toEqual({ diff: "shown", overview: "absent" });
    expect(reviewLayers("diff", true)).toEqual({ diff: "shown", overview: "hidden" });
  });

  it("unmounts both under a page that replaces the review", () => {
    for (const view of ["settings", "inbox", "export-review", "agent-output"] as const) {
      expect(reviewLayers(view, true)).toEqual({ diff: "absent", overview: "absent" });
    }
  });
});
