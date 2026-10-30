import { describe, expect, test } from "bun:test";
import { tourEmptyCopy } from "./tourEmpty";
import type { OtherBucketSnapshot } from "./types";

const other = (over: Partial<OtherBucketSnapshot>): OtherBucketSnapshot => ({
  view: "pr",
  has_triage: false,
  has_review: false,
  finding_count: 0,
  pillar_count: 0,
  tour_shared: false,
  same_diff: false,
  ...over,
});

describe("tourEmptyCopy", () => {
  test("a tab with one view just has no tour yet", () => {
    const copy = tourEmptyCopy(null);
    expect(copy.title).toBe("No tour yet");
    expect(copy.switchTo).toBeNull();
  });

  test("points at the other view's tour when it has one", () => {
    const copy = tourEmptyCopy(other({ view: "pr", pillar_count: 4 }));
    expect(copy.title).toBe("No tour for the Local branch view");
    expect(copy.body).toBe("The 4-pillar tour was generated from the PR diff view. Generate one here, or switch views.");
    expect(copy.switchTo).toBe("pr");
    expect(copy.switchLabel).toBe("Switch to PR diff");
  });

  test("names this view even when neither has a tour", () => {
    const copy = tourEmptyCopy(other({ view: "branch" }));
    expect(copy.title).toBe("No tour for the PR diff view");
    expect(copy.switchTo).toBeNull();
  });

  test("a shared tour is not offered as somewhere else", () => {
    expect(tourEmptyCopy(other({ pillar_count: 3, tour_shared: true })).switchTo).toBeNull();
  });
});
