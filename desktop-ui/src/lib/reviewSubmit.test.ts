import { describe, expect, it } from "vitest";
import {
  NO_PR_REASON,
  OWN_PR_REASON,
  PENDING_REVIEW_SINGLE_REASON,
  composerReviewLabel,
  decisionEvent,
  initialDecision,
  pendingCommentWhere,
  pendingReviewKey,
  pendingThreadIds,
  reviewGate,
  singleCommentBlockedReason,
  submitBlockedReason,
  submitFootnote,
  submittedToast,
  type PendingReview,
  type PendingReviewComment,
} from "./reviewSubmit";
import type { GithubStatusSnapshot } from "./types";

const gh = (over: Partial<GithubStatusSnapshot> = {}) =>
  ({ owner: "o", repo: "r", number: 7, state: "OPEN", is_authored_by_me: false, ...over }) as GithubStatusSnapshot;

const comment = (over: Partial<PendingReviewComment> = {}): PendingReviewComment => ({
  id: "PRRC_1",
  database_id: 11,
  path: "src/lib/a.ts",
  line: 12,
  start_line: null,
  side: "RIGHT",
  body: "why?",
  ...over,
});

describe("reviewGate", () => {
  it("disables with the reason when there is no PR", () => {
    expect(reviewGate(null, null)).toEqual({ enabled: false, reason: NO_PR_REASON });
  });

  it("disables a merged or closed PR, enables an open one", () => {
    expect(reviewGate(7, gh({ state: "MERGED" })).reason).toBe("This PR is merged");
    expect(reviewGate(7, gh()).enabled).toBe(true);
    // A status for another PR says nothing about this one.
    expect(reviewGate(8, gh({ state: "CLOSED" })).enabled).toBe(true);
  });
});

describe("submitBlockedReason", () => {
  it("refuses approving your own PR", () => {
    expect(submitBlockedReason("approve", 0, "", gh({ is_authored_by_me: true }))).toBe(OWN_PR_REASON);
    expect(initialDecision(gh({ is_authored_by_me: true }))).toBe("comment");
    expect(initialDecision(gh())).toBe("approve");
  });

  it("needs a summary or a comment for Comment and Request changes", () => {
    expect(submitBlockedReason("comment", 0, "  ", gh())).not.toBeNull();
    expect(submitBlockedReason("changes", 0, "", gh())).not.toBeNull();
    expect(submitBlockedReason("changes", 1, "", gh())).toBeNull();
    expect(submitBlockedReason("comment", 0, "lgtm", gh())).toBeNull();
    expect(submitBlockedReason("approve", 0, "", gh())).toBeNull();
  });
});

describe("labels", () => {
  it("maps decisions to GitHub events", () => {
    expect(decisionEvent("changes")).toBe("REQUEST_CHANGES");
    expect(decisionEvent("approve")).toBe("APPROVE");
  });

  it("words the toast and footnote by count", () => {
    expect(submittedToast("approve", 0)).toBe("Review submitted · Approved");
    expect(submittedToast("changes", 2)).toBe("Review submitted · Changes requested with 2 comments");
    expect(submitFootnote(1)).toBe("1 comment will be posted with your review");
    expect(submitFootnote(0)).toBe("Posts to GitHub as you");
    expect(composerReviewLabel(0)).toBe("Start a review");
    expect(composerReviewLabel(3)).toBe("Add review comment");
  });

  it("names a pending comment by file and range", () => {
    expect(pendingCommentWhere(comment())).toBe("a.ts:L12");
    expect(pendingCommentWhere(comment({ start_line: 10 }))).toBe("a.ts:L10–12");
    expect(pendingCommentWhere(comment({ line: null }))).toBe("a.ts");
  });
});

describe("pending identity", () => {
  it("derives the pulled thread ids", () => {
    const review: PendingReview = {
      pr_number: 7,
      review_id: 3,
      comments: [comment(), comment({ id: "PRRC_2", database_id: null })],
      viewer: "me",
      review_request: null,
    };
    expect([...pendingThreadIds(review)]).toEqual(["gh-11"]);
    expect(pendingThreadIds(null).size).toBe(0);
  });

  it("keys the draft by repo and PR", () => {
    expect(pendingReviewKey(null, gh())).toBeNull();
    expect(pendingReviewKey(7, gh())).toBe("o/r#7");
    expect(pendingReviewKey(9, gh())).toBe("#9");
  });
});

describe("singleCommentBlockedReason", () => {
  const open = { enabled: true, reason: null };
  const review = (over: Partial<PendingReview> = {}): PendingReview => ({
    pr_number: 7,
    review_id: null,
    comments: [],
    viewer: "me",
    review_request: null,
    ...over,
  });

  it("allows a single comment when no review has been started", () => {
    expect(singleCommentBlockedReason(open, null)).toBeNull();
    expect(singleCommentBlockedReason(open, review())).toBeNull();
  });

  it("refuses while a pending review exists, even once its comments are removed", () => {
    expect(singleCommentBlockedReason(open, review({ review_id: 5, comments: [comment()] }))).toBe(
      PENDING_REVIEW_SINGLE_REASON,
    );
    expect(singleCommentBlockedReason(open, review({ review_id: 5 }))).toBe(PENDING_REVIEW_SINGLE_REASON);
  });

  it("gives the gate's reason first", () => {
    expect(singleCommentBlockedReason({ enabled: false, reason: NO_PR_REASON }, review({ review_id: 5 }))).toBe(
      NO_PR_REASON,
    );
  });
});
