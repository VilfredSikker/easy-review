import type { GithubStatusSnapshot } from "./types";

/**
 * The reviewer's GitHub pending review, as `get_pending_review` returns it.
 * GitHub holds the draft, so this mirrors it rather than owning it.
 */
export interface PendingReview {
  pr_number: number;
  /** Null until a first "Start a review" comment opens one. */
  review_id: number | null;
  comments: PendingReviewComment[];
  viewer: string | null;
  review_request: { requested_by: string; requested_at: string } | null;
}

export interface PendingReviewComment {
  /** GraphQL id; what removal takes. */
  id: string;
  database_id: number | null;
  path: string;
  line: number | null;
  start_line: number | null;
  side: "LEFT" | "RIGHT" | string;
  body: string;
}

/** The composer's selection, in the line numbering of its side. */
export interface ReviewLineTarget {
  file: string;
  lineNum: number;
  lineNumEnd: number | null;
  side: "LEFT" | "RIGHT" | null;
}

export type ReviewDecision = "comment" | "approve" | "changes";

export const REVIEW_DECISIONS: readonly {
  id: ReviewDecision;
  label: string;
  description: string;
  event: "COMMENT" | "APPROVE" | "REQUEST_CHANGES";
}[] = [
  { id: "comment", label: "Comment", description: "Submit general feedback without explicit approval.", event: "COMMENT" },
  { id: "approve", label: "Approve", description: "Give your approval to merge these changes.", event: "APPROVE" },
  { id: "changes", label: "Request changes", description: "Submit feedback that must be addressed before merging.", event: "REQUEST_CHANGES" },
];

const DECISION_DONE: Record<ReviewDecision, string> = {
  comment: "Commented",
  approve: "Approved",
  changes: "Changes requested",
};

export const NO_PR_REASON = "No pull request for this branch — open one to review on GitHub";
export const OWN_PR_REASON = "GitHub does not allow approving your own PR";

export interface Gate {
  enabled: boolean;
  /** Why it is disabled, for the tooltip. */
  reason: string | null;
}

/**
 * Whether anything can be sent to a GitHub review here. A local branch with
 * no PR has nowhere to post, so the Review button and the composer's GitHub
 * actions stay visible but disabled with the reason.
 */
export function reviewGate(prNumber: number | null, github: GithubStatusSnapshot | null | undefined): Gate {
  if (prNumber == null) return { enabled: false, reason: NO_PR_REASON };
  if (github && github.number === prNumber && github.state !== "OPEN") {
    return { enabled: false, reason: `This PR is ${github.state.toLowerCase()}` };
  }
  return { enabled: true, reason: null };
}

export const PENDING_REVIEW_SINGLE_REASON =
  "You have a pending review — add this to it or submit the review first";

/**
 * Why "Add single comment" is refused. GitHub keeps one pending review per
 * reviewer per PR and rejects a standalone review comment while it exists,
 * even an empty one, so the button waits until the review is submitted.
 */
export function singleCommentBlockedReason(gate: Gate, review: PendingReview | null): string | null {
  if (!gate.enabled) return gate.reason;
  if (review?.review_id != null) return PENDING_REVIEW_SINGLE_REASON;
  return null;
}

/** Why one decision cannot be picked, if it cannot. */
export function decisionBlockedReason(
  decision: ReviewDecision,
  github: GithubStatusSnapshot | null | undefined,
): string | null {
  if (decision === "approve" && github?.is_authored_by_me) return OWN_PR_REASON;
  return null;
}

/**
 * Why Submit is refused. GitHub rejects a Comment or Request changes review
 * that says nothing, so one needs a summary or a pending line comment.
 */
export function submitBlockedReason(
  decision: ReviewDecision,
  pendingCount: number,
  summary: string,
  github: GithubStatusSnapshot | null | undefined,
): string | null {
  const blocked = decisionBlockedReason(decision, github);
  if (blocked) return blocked;
  if (decision !== "approve" && pendingCount === 0 && summary.trim() === "") {
    return "Add a summary or a line comment first";
  }
  return null;
}

/** The decision a fresh popover starts on: Approve, unless GitHub would refuse it. */
export function initialDecision(github: GithubStatusSnapshot | null | undefined): ReviewDecision {
  return decisionBlockedReason("approve", github) ? "comment" : "approve";
}

export function decisionEvent(decision: ReviewDecision): string {
  return REVIEW_DECISIONS.find((d) => d.id === decision)?.event ?? "COMMENT";
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export function submittedToast(decision: ReviewDecision, count: number): string {
  return `Review submitted · ${DECISION_DONE[decision]}${count ? ` with ${plural(count, "comment")}` : ""}`;
}

export function addedToast(count: number): string {
  return `Added to your pending review · ${count} pending, submit from Review`;
}

export function submitFootnote(count: number): string {
  return count ? `${plural(count, "comment")} will be posted with your review` : "Posts to GitHub as you";
}

/** The composer's review button: the first comment starts the review. */
export function composerReviewLabel(count: number): string {
  return count ? "Add review comment" : "Start a review";
}

/** `file.ts:L12` or `file.ts:L10–12`, as the popover lists a pending comment. */
export function pendingCommentWhere(c: PendingReviewComment): string {
  const name = c.path.split("/").pop() || c.path;
  if (c.line == null) return name;
  const range = c.start_line != null && c.start_line < c.line ? `L${c.start_line}–${c.line}` : `L${c.line}`;
  return `${name}:${range}`;
}

/**
 * Thread ids the diff would give these comments once pulled. A pulled GitHub
 * comment's id is `gh-<rest id>`, so this is how a drawn thread knows it is
 * still pending.
 */
export function pendingThreadIds(review: PendingReview | null): Set<string> {
  const ids = new Set<string>();
  for (const c of review?.comments ?? []) {
    if (c.database_id != null) ids.add(`gh-${c.database_id}`);
  }
  return ids;
}

/** Which PR a pending review belongs to, so a tab switch never shows another PR's draft. */
export function pendingReviewKey(
  prNumber: number | null,
  github: GithubStatusSnapshot | null | undefined,
): string | null {
  if (prNumber == null) return null;
  const repo = github && github.number === prNumber ? `${github.owner}/${github.repo}` : "";
  return `${repo}#${prNumber}`;
}
