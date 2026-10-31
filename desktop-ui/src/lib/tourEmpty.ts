import type { OtherBucketSnapshot } from "./types";

export interface TourEmptyCopy {
  title: string;
  body: string;
  /** The other view of a local PR tab holds a tour; offer to go read it. */
  switchTo: "pr" | "branch" | null;
  /** Label for the switch button. */
  switchLabel: string | null;
}

const VIEW_LABEL = { pr: "PR diff", branch: "Local branch" } as const;

/**
 * What the Tour page says when this view has no tour. A local PR tab keeps a
 * tour per view (ADR 0004), so when the other view has one the copy says so
 * and offers it, rather than implying no tour exists anywhere. A shared tour
 * (ADR 0025) would already be this view's, so it never reaches here.
 */
export function tourEmptyCopy(other: OtherBucketSnapshot | null | undefined): TourEmptyCopy {
  const elsewhere = other && other.pillar_count > 0 && !other.tour_shared ? other : null;
  const here = other ? VIEW_LABEL[other.view === "pr" ? "branch" : "pr"] : null;
  const title = here ? `No tour for the ${here} view` : "No tour yet";
  if (!elsewhere) {
    return {
      title,
      body: "A tour splits the change into pillars, the few ideas it is made of, and walks them in reading order. Generate one, or read the Diff.",
      switchTo: null,
      switchLabel: null,
    };
  }
  const there = VIEW_LABEL[elsewhere.view];
  const pillars = `${elsewhere.pillar_count}-pillar`;
  return {
    title,
    body: `The ${pillars} tour was generated from the ${there} view. Generate one here, or switch views.`,
    switchTo: elsewhere.view,
    switchLabel: `Switch to ${there}`,
  };
}
