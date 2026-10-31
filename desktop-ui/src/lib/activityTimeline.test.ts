import { describe, expect, it } from "bun:test";
import {
  AVATAR_TOKENS,
  avatarToken,
  buildActivity,
  initials,
  openedText,
} from "./activityTimeline";
import type { AiSnapshot, FlatFinding, GithubStatusSnapshot } from "./types";

function gh(over: Partial<GithubStatusSnapshot> = {}): GithubStatusSnapshot {
  return {
    owner: "o",
    repo: "r",
    number: 7,
    url: "https://github.com/o/r/pull/7",
    state: "OPEN",
    is_draft: false,
    title: "t",
    body: "",
    author: "sune",
    head_ref: "fix",
    base_ref: "main",
    review_decision: null,
    mergeable: null,
    labels: [],
    checks: [],
    comments_count: 0,
    reviews_count: 0,
    recent_comments: [],
    recent_reviews: [],
    last_updated: null,
    is_authored_by_me: false,
    created_at: "2026-10-30T09:00:00Z",
    commit_count: 1,
    reviewers: [],
    ...over,
  };
}

function finding(severity: FlatFinding["severity"]): FlatFinding {
  return { severity } as FlatFinding;
}

type AiInput = Parameters<typeof buildActivity>[1];
function ai(over: Partial<AiSnapshot> = {}): AiInput {
  return {
    summary_markdown: "Looks fine.",
    findings: [],
    fresh: true,
    has_review_json: true,
    review_created_at: "2026-10-30T09:05:00Z",
    ...over,
  };
}

describe("buildActivity", () => {
  it("orders opened, requests, AI review and reviews by time", () => {
    const { events } = buildActivity(
      gh({
        reviewers: [
          { reviewer: "martin", is_team: false, requested: true, state: null },
          { reviewer: "maria", is_team: false, requested: false, state: "COMMENTED" },
        ],
        recent_reviews: [
          { author: "martin", state: "APPROVED", body: "", submitted_at: "2026-10-30T09:10:00Z" },
        ],
      }),
      ai(),
    );
    expect(events.map((e) => e.kind)).toEqual(["opened", "requested", "ai-review", "review"]);
  });

  it("puts undated review requests right after opened", () => {
    const { events } = buildActivity(
      gh({
        reviewers: [{ reviewer: "team/backend", is_team: true, requested: true, state: null }],
        recent_comments: [
          { author: "a", body: "early", created_at: "2026-10-30T09:00:00Z", url: "u1" },
        ],
      }),
      null,
    );
    expect(events.map((e) => e.kind)).toEqual(["opened", "requested", "comment"]);
  });

  it("drops pending and unknown review states", () => {
    const { events } = buildActivity(
      gh({
        recent_reviews: [
          { author: "me", state: "PENDING", body: "draft", submitted_at: "" },
          { author: "x", state: "WEIRD", body: "", submitted_at: "2026-10-30T09:02:00Z" },
          { author: "y", state: "CHANGES_REQUESTED", body: " fix it ", submitted_at: "2026-10-30T09:03:00Z" },
        ],
      }),
      null,
    );
    const reviews = events.filter((e) => e.kind === "review");
    expect(reviews).toHaveLength(1);
    expect(reviews[0]).toMatchObject({ actor: "y", state: "changes", body: "fix it" });
  });

  it("omits opened when GitHub sent no createdAt", () => {
    const { events } = buildActivity(gh({ created_at: null }), null);
    expect(events).toEqual([]);
  });

  it("counts blocking as open high-severity findings", () => {
    const { events } = buildActivity(null, ai({ findings: [finding("high"), finding("med"), finding("high")] }));
    expect(events).toHaveLength(1);
    expect(events[0]).toMatchObject({ kind: "ai-review", blocking: 2, findings: 3 });
  });

  it("leaves the AI card out when the bucket has no review", () => {
    const { events } = buildActivity(null, ai({ has_review_json: false, summary_markdown: null }));
    expect(events).toEqual([]);
  });

  it("sorts an undated AI review last", () => {
    const { events } = buildActivity(
      gh({ recent_comments: [{ author: "a", body: "b", created_at: "2026-10-30T11:00:00Z", url: "u" }] }),
      ai({ review_created_at: "" }),
    );
    expect(events.map((e) => e.kind)).toEqual(["opened", "comment", "ai-review"]);
    expect(events[2].at).toBeNull();
  });

  it("reports comments GitHub has beyond the recent ones", () => {
    const timeline = buildActivity(
      gh({ comments_count: 5, recent_comments: [{ author: "a", body: "b", created_at: "2026-10-30T09:30:00Z", url: "u" }] }),
      null,
    );
    expect(timeline.olderComments).toBe(4);
  });
});

describe("openedText", () => {
  it("pluralises the commit count and drops an unknown one", () => {
    expect(openedText("sune", 1)).toBe("sune opened with 1 commit");
    expect(openedText("sune", 3)).toBe("sune opened with 3 commits");
    expect(openedText("sune", null)).toBe("sune opened this PR");
  });
});

describe("avatars", () => {
  it("keeps one token per name, from the theme set", () => {
    expect(avatarToken("Martin")).toBe(avatarToken("martin"));
    expect(AVATAR_TOKENS).toContain(avatarToken("sune") as (typeof AVATAR_TOKENS)[number]);
  });

  it("takes up to two initials", () => {
    expect(initials("Sune Ørnemark Lægdsmand")).toBe("SØ");
    expect(initials("maria-c")).toBe("MC");
    expect(initials("owner/backend")).toBe("B");
    expect(initials("")).toBe("?");
  });
});
