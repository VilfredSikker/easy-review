import { describe, expect, it } from "bun:test";
import type { AppSnapshot, TabSummary } from "../types";
import {
  ReviewPageMemory,
  currentReviewPage,
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
  it("restores the Overview on the tab that was left on it", () => {
    const m = new ReviewPageMemory();
    expect(m.follow("a", "diff")).toBeNull();
    // Reader opens the Overview on a, then switches to b.
    expect(m.follow("b", "overview")).toBe("diff");
    expect(m.follow("a", "diff")).toBe("overview");
    expect(m.follow("b", "overview")).toBe("diff");
  });

  it("forgets the Overview once the tab is left on Tour or Diff", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "diff");
    m.follow("b", "overview");
    m.follow("a", "diff");
    expect(m.follow("b", "diff")).toBeNull();
    expect(m.follow("a", "diff")).toBeNull();
  });

  it("leaves a non-review main view open across tab switches", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "overview");
    expect(m.follow("b", "settings")).toBeNull();
    expect(m.follow("a", "inbox")).toBeNull();
  });

  it("does nothing while the tab stays the same", () => {
    const m = new ReviewPageMemory();
    m.follow("a", "diff");
    expect(m.follow("a", "overview")).toBeNull();
  });
});
