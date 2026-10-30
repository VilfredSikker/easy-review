import type { ThreadSnapshot } from "$lib/types";
import { visibleCommentThreads, type CommentVisibility } from "$lib/commentVisibility";

/**
 * What the Comments tab lists: GitHub comments under the diff's own hide
 * filters (so the panel never shows a thread the diff is hiding), then every
 * question and note. The tab count, the collapsed rail and the list share it.
 */
export function panelThreads(
  threads: ThreadSnapshot[] | null | undefined,
  visibility: CommentVisibility,
  files?: Parameters<typeof visibleCommentThreads>[2],
): ThreadSnapshot[] {
  const github = visibleCommentThreads(threads, visibility, files);
  return [...github, ...(threads ?? []).filter((t) => t.kind !== "comment")];
}

export type ThreadKind = ThreadSnapshot["kind"];
export type KindFilter = "all" | ThreadKind;

export const KIND_FILTERS: readonly { id: KindFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "comment", label: "GitHub" },
  { id: "question", label: "Questions" },
  { id: "note", label: "Notes" },
];

export const KIND_LABEL: Record<ThreadKind, string> = {
  comment: "Comment",
  question: "Question",
  note: "Note",
};

export type KindCounts = Record<KindFilter, number>;

export function kindCounts(threads: readonly ThreadSnapshot[]): KindCounts {
  const counts: KindCounts = { all: threads.length, comment: 0, question: 0, note: 0 };
  for (const t of threads) counts[t.kind]++;
  return counts;
}

/**
 * Unresolved first, then in the diff's file order, then by line — the order a
 * reader meets them scrolling the diff. A file the diff does not list (a filter
 * hid it) sorts after every listed one rather than jumping to the top.
 */
export function commentRows(
  threads: readonly ThreadSnapshot[],
  kind: KindFilter,
  fileOrder: readonly string[],
): ThreadSnapshot[] {
  const rank = new Map(fileOrder.map((path, i) => [path, i]));
  const fileRank = (path: string) => rank.get(path) ?? fileOrder.length;
  return threads
    .filter((t) => kind === "all" || t.kind === kind)
    .sort(
      (a, b) =>
        Number(a.resolved) - Number(b.resolved) ||
        fileRank(a.file) - fileRank(b.file) ||
        a.file.localeCompare(b.file) ||
        a.line - b.line,
    );
}

/** `12` or `12–18`: the lines a thread is anchored to. */
export function lineRange(line: number, lineEnd?: number | null): string {
  return lineEnd != null && lineEnd !== line ? `${line}–${lineEnd}` : String(line);
}

export interface ReplyTally {
  replies: number;
  ai: number;
}

export function replyTally(thread: Pick<ThreadSnapshot, "replies">): ReplyTally {
  return { replies: thread.replies.length, ai: thread.replies.filter((r) => r.kind === "ai").length };
}

/** Comments go to GitHub; questions and notes never leave the machine. */
export function threadAudience(kind: ThreadKind): string {
  return kind === "comment" ? "GitHub" : "only you";
}

/** First line of a markdown body, flattened for a one-line row. */
export function threadExcerpt(body: string, max = 110): string {
  const flat = body.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 2)}…` : flat;
}

const NOTES_SUBTAB_KEY = "notesPanelSubTab";
export const COMMENTS_KIND_KEY = "er.rightPanel.commentsKind";

/** Reads the kind filter, carrying over the old Notes tab's sub-filter once. */
export function loadKindFilter(storage: Pick<Storage, "getItem" | "setItem" | "removeItem">): KindFilter {
  const current = storage.getItem(COMMENTS_KIND_KEY);
  if (current === "all" || current === "comment" || current === "question" || current === "note") {
    return current;
  }
  const legacy = storage.getItem(NOTES_SUBTAB_KEY);
  const LEGACY: Record<string, KindFilter> = { notes: "note", questions: "question" };
  const migrated: KindFilter = (legacy && LEGACY[legacy]) || "all";
  storage.setItem(COMMENTS_KIND_KEY, migrated);
  storage.removeItem(NOTES_SUBTAB_KEY);
  return migrated;
}
