import type { AiSnapshot, GithubStatusSnapshot } from "./types";

/**
 * One row of the Overview's Activity timeline. `gh pr view --json` has no
 * timeline (ADR 0026), so every event is rebuilt from fields it does return:
 * the PR's `createdAt`, its review requests, submitted reviews and comments,
 * plus the local AI review. Events GitHub alone knows about (force-pushes,
 * label changes, request times) are deliberately absent rather than guessed.
 */
export type ActivityEvent =
  | { kind: "opened"; key: string; at: string; actor: string; commits: number | null }
  | { kind: "requested"; key: string; at: null; reviewer: string; isTeam: boolean }
  | { kind: "review"; key: string; at: string; actor: string; state: ReviewState; body: string }
  | { kind: "comment"; key: string; at: string; actor: string; body: string; url: string }
  | { kind: "ai-review"; key: string; at: string | null; summary: string; blocking: number; findings: number; fresh: boolean };

export type ReviewState = "approved" | "changes" | "commented" | "dismissed";

export interface ActivityTimeline {
  events: ActivityEvent[];
  /** Comments GitHub counts that `recent_comments` did not carry. */
  olderComments: number;
}

const REVIEW_STATES: Record<string, ReviewState> = {
  APPROVED: "approved",
  CHANGES_REQUESTED: "changes",
  COMMENTED: "commented",
  DISMISSED: "dismissed",
};

/** What the review row says after the reviewer's name. */
export const REVIEW_VERB: Record<ReviewState, string> = {
  approved: "approved",
  changes: "requested changes",
  commented: "reviewed",
  dismissed: "had a review dismissed",
};

function time(iso: string | null | undefined): number {
  const t = iso ? Date.parse(iso) : Number.NaN;
  return Number.isNaN(t) ? Number.POSITIVE_INFINITY : t;
}

type AiInput = Pick<
  AiSnapshot,
  "summary_markdown" | "findings" | "fresh" | "has_review_json" | "review_created_at"
>;

/**
 * The AI review as one card. It is the bucket's review, not a person's: the
 * lenses that wrote it stay on the findings (CONTEXT.md, Lens), so the card
 * never takes a reviewer's name. "Blocking" is the open high-severity count.
 */
function aiReviewEvent(ai: AiInput | null | undefined): ActivityEvent | null {
  if (!ai) return null;
  const summary = ai.summary_markdown?.trim() ?? "";
  if (!ai.has_review_json && !summary && ai.findings.length === 0) return null;
  return {
    kind: "ai-review",
    key: "ai-review",
    at: ai.review_created_at || null,
    summary,
    blocking: ai.findings.filter((f) => f.severity === "high").length,
    findings: ai.findings.length,
    fresh: ai.fresh,
  };
}

function githubEvents(gh: GithubStatusSnapshot): ActivityEvent[] {
  const events: ActivityEvent[] = [];
  if (gh.created_at) {
    events.push({
      kind: "opened",
      key: "opened",
      at: gh.created_at,
      actor: gh.author,
      commits: gh.commit_count ?? null,
    });
  }
  for (const r of gh.reviewers ?? []) {
    if (!r.requested) continue;
    events.push({ kind: "requested", key: `req:${r.reviewer}`, at: null, reviewer: r.reviewer, isTeam: r.is_team });
  }
  gh.recent_reviews.forEach((r, i) => {
    const state = REVIEW_STATES[r.state];
    // A PENDING review is the viewer's own unsubmitted draft: not activity yet.
    if (!state || !r.submitted_at) return;
    events.push({ kind: "review", key: `rev:${i}:${r.author}`, at: r.submitted_at, actor: r.author, state, body: r.body.trim() });
  });
  gh.recent_comments.forEach((c, i) => {
    events.push({ kind: "comment", key: `com:${i}:${c.url}`, at: c.created_at, actor: c.author, body: c.body.trim(), url: c.url });
  });
  return events;
}

/**
 * Oldest first, like GitHub's own conversation. A review request has no time
 * on `gh pr view`, so it sits right after "opened", where the prototype and
 * most PRs put it; an undated AI review goes last, as the newest thing local.
 */
function sortKey(e: ActivityEvent, openedAt: number): number {
  if (e.kind === "requested") return openedAt;
  return time(e.at);
}

const KIND_ORDER: Record<ActivityEvent["kind"], number> = {
  opened: 0,
  requested: 1,
  review: 2,
  comment: 2,
  "ai-review": 3,
};

export function buildActivity(
  github: GithubStatusSnapshot | null | undefined,
  ai: AiInput | null | undefined,
): ActivityTimeline {
  const events = github ? githubEvents(github) : [];
  const aiEvent = aiReviewEvent(ai);
  if (aiEvent) events.push(aiEvent);
  const openedAt = time(github?.created_at);
  const sorted = events
    .map((e, i) => ({ e, i, t: sortKey(e, openedAt) }))
    .sort((a, b) => a.t - b.t || KIND_ORDER[a.e.kind] - KIND_ORDER[b.e.kind] || a.i - b.i)
    .map(({ e }) => e);
  const olderComments = github ? Math.max(0, github.comments_count - github.recent_comments.length) : 0;
  return { events: sorted, olderComments };
}

/** "opened with 2 commits": the count is dropped when GitHub did not send it. */
export function openedText(actor: string, commits: number | null): string {
  if (commits == null || commits <= 0) return `${actor} opened this PR`;
  return `${actor} opened with ${commits} commit${commits === 1 ? "" : "s"}`;
}

/**
 * Avatar tints, cycled by a hash of the name so a person keeps one colour on
 * every row. Semantic accents, never the prototype's fixed hex (ADR 0030):
 * they follow the theme, and the risk/diff colours stay out so an avatar never
 * reads as a severity.
 */
export const AVATAR_TOKENS = [
  "--color-action",
  "--color-periwinkle",
  "--color-info",
  "--color-emphasis",
  "--color-success",
  "--color-comment",
] as const;

export function avatarToken(name: string): string {
  let h = 0;
  for (const ch of name.toLowerCase()) h = (h * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return AVATAR_TOKENS[h % AVATAR_TOKENS.length];
}

/** Up to two letters: "Sune Ørnemark" → "SØ", "maria-c" → "MC", "owner/backend" → "B". */
export function initials(name: string): string {
  const local = name.includes("/") ? (name.split("/").pop() ?? name) : name;
  const parts = local.split(/[\s._-]+/u).filter(Boolean);
  const letters = parts.slice(0, 2).map((p) => [...p][0] ?? "");
  return letters.join("").toUpperCase() || "?";
}
