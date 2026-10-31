import type { GhCommentSummary, GhReviewSummary, GhReviewerSummary, ThreadSnapshot } from "./types";

/** One thing a reviewer said on the PR, as the reviewer popover lists it. */
export interface ReviewerEntry {
  key: string;
  /** `review`: the review's own body or verdict; `pr`: a conversation comment. */
  kind: "review" | "pr" | "comment" | "reply";
  text: string;
  at: string;
  /** Inline entries only: where the thread sits and its id for the jump. */
  file: string | null;
  line: number | null;
  lineEnd: number | null;
  threadId: string | null;
}

/**
 * GitHub names the same account `copilot-pull-request-reviewer` on a review,
 * `Copilot` on the reviewer list and `copilot-pull-request-reviewer[bot]` on a
 * comment. Matching on a normalised login keeps those one person.
 */
export function sameAuthor(a: string, b: string): boolean {
  const norm = (s: string) =>
    s.trim().toLowerCase().replace(/\[bot\]$/, "").replace(/^copilot-pull-request-reviewer$/, "copilot");
  return norm(a) === norm(b);
}

const STATE_LABEL: Record<string, string> = {
  APPROVED: "approved",
  CHANGES_REQUESTED: "changes requested",
  COMMENTED: "commented",
  DISMISSED: "dismissed",
};

/** The reviewer row's status word: their verdict, or "requested" before they review. */
export function reviewerStateLabel(reviewer: Pick<GhReviewerSummary, "state" | "requested">): string {
  if (reviewer.state) return STATE_LABEL[reviewer.state] ?? reviewer.state.toLowerCase().replace(/_/g, " ");
  return reviewer.requested ? "requested" : "";
}

/** The status button's tone: approval is good news, a change request is a blocker. */
export function reviewerStateTone(state: string | null): "ok" | "block" | "neutral" {
  if (state === "APPROVED") return "ok";
  if (state === "CHANGES_REQUESTED") return "block";
  return "neutral";
}

export interface ReviewerSources {
  reviews?: readonly GhReviewSummary[];
  comments?: readonly GhCommentSummary[];
  threads?: readonly ThreadSnapshot[] | null;
}

/**
 * Everything one reviewer left: review verdicts and bodies, newest first, then
 * conversation comments, then inline comments and replies in file and line
 * order. Only GitHub threads count; questions and notes never left the machine,
 * and an AI reply inside a thread is not the reviewer's.
 */
export function reviewerEntries(login: string, src: ReviewerSources): ReviewerEntry[] {
  const reviews: ReviewerEntry[] = (src.reviews ?? [])
    .filter((r) => sameAuthor(r.author, login))
    .sort((a, b) => b.submitted_at.localeCompare(a.submitted_at))
    .map((r, i) => ({
      key: `review:${r.submitted_at}:${i}`,
      kind: "review",
      text: r.body.trim() || capitalise(STATE_LABEL[r.state] ?? r.state.toLowerCase()),
      at: r.submitted_at,
      file: null,
      line: null,
      lineEnd: null,
      threadId: null,
    }));

  const pr: ReviewerEntry[] = (src.comments ?? [])
    .filter((c) => sameAuthor(c.author, login))
    .sort((a, b) => b.created_at.localeCompare(a.created_at))
    .map((c) => ({
      key: `pr:${c.url || c.created_at}`,
      kind: "pr",
      text: c.body.trim(),
      at: c.created_at,
      file: null,
      line: null,
      lineEnd: null,
      threadId: null,
    }));

  const inline: ReviewerEntry[] = [];
  const threads = [...(src.threads ?? [])]
    .filter((t) => t.kind === "comment")
    .sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line);
  for (const t of threads) {
    const where = { file: t.file, line: t.line, lineEnd: t.line_end ?? null, threadId: t.id };
    if (sameAuthor(t.root.author, login)) {
      inline.push({ key: `c:${t.id}`, kind: "comment", text: t.root.body_markdown.trim(), at: t.root.timestamp, ...where });
    }
    for (const r of t.replies) {
      if (r.kind === "ai" || !sameAuthor(r.author, login)) continue;
      inline.push({ key: `r:${t.id}:${r.id}`, kind: "reply", text: r.body_markdown.trim(), at: r.timestamp, ...where });
    }
  }

  return [...reviews, ...pr, ...inline];
}

function capitalise(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}

/** `schema.sql:12` / `schema.sql:12–18`; `review` or `PR` for entries off the diff. */
export function entryWhere(entry: ReviewerEntry): string {
  if (entry.file == null || entry.line == null) return entry.kind === "review" ? "review" : "PR";
  const name = entry.file.split("/").pop() ?? entry.file;
  const range = entry.lineEnd != null && entry.lineEnd !== entry.line ? `${entry.line}–${entry.lineEnd}` : String(entry.line);
  return `${name}:${range}`;
}
