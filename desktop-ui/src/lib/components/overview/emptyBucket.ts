import type { AppSnapshot, OtherBucketSnapshot } from "$lib/types";
import { activeDiffSource } from "$lib/sourceToggle";

export interface EmptyBucketCopy {
  title: string;
  body: string;
  /** The view the artifacts are in, for the switch button. */
  otherView: OtherBucketSnapshot["view"];
  switchLabel: string;
}

const VIEW_NAME: Record<OtherBucketSnapshot["view"], string> = {
  pr: "PR diff view",
  branch: "Local branch view",
};

/** "a triage, 4 findings and a 4-pillar tour" — what the other view holds. */
function holdings(other: OtherBucketSnapshot): string[] {
  const parts: string[] = [];
  if (other.has_triage) parts.push("a triage");
  if (other.has_review) {
    parts.push(
      other.finding_count > 0
        ? `${other.finding_count} finding${other.finding_count === 1 ? "" : "s"}`
        : "a review with no findings",
    );
  }
  // A shared tour is this view's tour too (ADR 0025); listing it as "over
  // there" would say it is missing here.
  if (other.pillar_count > 0 && !other.tour_shared) parts.push(`a ${other.pillar_count}-pillar tour`);
  return parts;
}

function joinList(parts: readonly string[], conjunction: "and" | "or"): string {
  if (parts.length <= 1) return parts.join("");
  return `${parts.slice(0, -1).join(", ")} ${conjunction} ${parts[parts.length - 1]}`;
}

type BucketSnapshot = Pick<AppSnapshot, "mode" | "ai" | "tour" | "other_bucket">;

/**
 * The card a view shows in place of its triage, findings and tour when its own
 * bucket is empty but the other view of the same PR tab is not (ADR 0004).
 * Null whenever this view has a triage or a review of its own: then the normal
 * sections draw, each with its own empty state.
 *
 * The tour is judged by `tour.available`, never by the bucket, because a tour
 * is reused across the two views on a matching diff (ADR 0025): the title may
 * only say "no tour" when there really is none on screen.
 */
export function emptyBucketCopy(snap: BucketSnapshot | null): EmptyBucketCopy | null {
  const other = snap?.other_bucket;
  if (!snap || !other) return null;
  const ai = snap.ai;
  const hasReview = ai.has_review_json || !!ai.summary_markdown?.trim() || ai.findings.length > 0;
  if (ai.triage || hasReview) return null;

  const here = activeDiffSource(snap.mode, snap.tour?.scope) === "pr" ? "pr" : "branch";
  if (here === other.view) return null;

  const missing = ["triage", "review"];
  if (!snap.tour?.available) missing.push("tour");
  const title = `No ${joinList(missing, "or")} in the ${VIEW_NAME[here]}`;

  const held = holdings(other);
  const there = held.length > 0 ? `The ${VIEW_NAME[other.view]} has ${joinList(held, "and")}` : `The ${VIEW_NAME[other.view]} has no artifacts either`;
  const shared = other.tour_shared ? " The tour is shared: both views show the same diff." : "";
  return {
    title,
    body: `Review artifacts belong to the view that produced them. ${there}; this view has its own, empty bucket.${shared}`,
    otherView: other.view,
    switchLabel: other.view === "pr" ? "Switch to PR diff" : "Switch to Local branch",
  };
}
