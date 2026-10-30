import type { InboxItemSnapshot, PrInfo, ProjectSnapshot, TabSummary } from "$lib/types";
import {
  INBOX_CATEGORY_LABELS,
  INBOX_CATEGORY_ORDER,
  inboxItemCategory,
  inboxItemProjectId,
  type InboxCategoryId,
} from "$lib/inboxCategories";

/**
 * The inbox lists one row per PR, not per event: five comments on one PR are
 * one thing to go and look at. A thread is every inbox item that points at
 * the same PR (or branch, for CI and AI items that only know a branch).
 */
export interface InboxThread {
  key: string;
  /** Oldest first, the order the detail timeline reads in. */
  items: InboxItemSnapshot[];
  latest: InboxItemSnapshot;
  projectId: string | null;
  prNumber: number | null;
  unreadIds: string[];
  categories: InboxCategoryId[];
}

export type InboxReadFilter = "all" | "unread" | "read";

export interface InboxThreadFilter {
  read: InboxReadFilter;
  projectId: "all" | string;
  category: "all" | InboxCategoryId;
}

/** Theme token per type, so the row tint follows the theme (ADR 0030). */
export const INBOX_CATEGORY_TONE: Record<InboxCategoryId, string> = {
  pr_comment: "comment",
  comment_reply: "comment",
  mention: "comment",
  review_received: "add-fg",
  approved: "add-fg",
  changes_requested: "warning",
  review_requested: "accent",
  ci: "del-fg",
  lifecycle: "periwinkle",
  ai: "ai",
  other: "muted",
};

/** Types whose body is something a person wrote or a failure to read, worth quoting. */
const QUOTED_CATEGORIES: ReadonlySet<InboxCategoryId> = new Set<InboxCategoryId>([
  "pr_comment",
  "comment_reply",
  "mention",
  "changes_requested",
  "ci",
  "ai",
]);

/** Categories that only reach you about a PR you opened. */
const AUTHOR_CATEGORIES: ReadonlySet<InboxCategoryId> = new Set<InboxCategoryId>([
  "pr_comment",
  "review_received",
  "approved",
  "changes_requested",
]);

export function inboxToneStyle(tone: string): string {
  return `color: var(--color-${tone}); background: color-mix(in srgb, var(--color-${tone}) 13%, transparent);`;
}

export function isQuotedCategory(category: InboxCategoryId): boolean {
  return QUOTED_CATEGORIES.has(category);
}

function threadKey(item: InboxItemSnapshot, projectId: string | null): string {
  const t = item.target;
  const scope = projectId ?? t.remote?.toLowerCase() ?? t.repo_root ?? "";
  if (t.pr_number != null) return `pr:${scope}#${t.pr_number}`;
  if (t.branch) return `branch:${scope}:${t.branch}`;
  return `item:${item.id}`;
}

/** Newest activity first. */
export function groupInboxThreads(
  items: InboxItemSnapshot[],
  projects: ProjectSnapshot[],
): InboxThread[] {
  const byKey = new Map<string, { projectId: string | null; items: InboxItemSnapshot[] }>();
  for (const item of items) {
    const projectId = inboxItemProjectId(item, projects);
    const key = threadKey(item, projectId);
    const entry = byKey.get(key);
    if (entry) entry.items.push(item);
    else byKey.set(key, { projectId, items: [item] });
  }
  const threads: InboxThread[] = [];
  for (const [key, entry] of byKey) {
    const sorted = [...entry.items].sort((a, b) => a.created_at_ms - b.created_at_ms);
    const latest = sorted[sorted.length - 1];
    const seen = new Set(sorted.map(inboxItemCategory));
    threads.push({
      key,
      items: sorted,
      latest,
      projectId: entry.projectId,
      prNumber: latest.target.pr_number ?? null,
      unreadIds: sorted.filter((i) => i.read_at_ms == null).map((i) => i.id),
      categories: INBOX_CATEGORY_ORDER.filter((c) => seen.has(c)),
    });
  }
  return threads.sort((a, b) => b.latest.created_at_ms - a.latest.created_at_ms);
}

export function isThreadUnread(thread: InboxThread): boolean {
  return thread.unreadIds.length > 0;
}

export function threadLatestCategory(thread: InboxThread): InboxCategoryId {
  return inboxItemCategory(thread.latest);
}

export function filterInboxThreads(threads: InboxThread[], f: InboxThreadFilter): InboxThread[] {
  return threads.filter((t) => {
    if (f.projectId !== "all" && t.projectId !== f.projectId) return false;
    if (f.read === "unread" && !isThreadUnread(t)) return false;
    if (f.read === "read" && isThreadUnread(t)) return false;
    if (f.category !== "all" && !t.categories.includes(f.category)) return false;
    return true;
  });
}

export interface InboxReadCounts {
  all: number;
  unread: number;
  read: number;
}

export function inboxReadCounts(threads: InboxThread[]): InboxReadCounts {
  const unread = threads.filter(isThreadUnread).length;
  return { all: threads.length, unread, read: threads.length - unread };
}

export interface InboxTypeChip {
  category: InboxCategoryId;
  label: string;
  count: number;
}

/** A PR counts once under every type it has an event of. */
export function inboxTypeChips(threads: InboxThread[]): InboxTypeChip[] {
  return INBOX_CATEGORY_ORDER.flatMap((category) => {
    const count = threads.filter((t) => t.categories.includes(category)).length;
    return count > 0 ? [{ category, label: INBOX_CATEGORY_LABELS[category], count }] : [];
  });
}

export type InboxDay = "Today" | "Yesterday" | "Earlier";

export function inboxDay(ms: number, now: number = Date.now()): InboxDay {
  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  const today = start.getTime();
  if (ms >= today) return "Today";
  start.setDate(start.getDate() - 1);
  return ms >= start.getTime() ? "Yesterday" : "Earlier";
}

/** Rows with a day label on the first row of each day. */
export function withDaySeparators(
  threads: InboxThread[],
  now: number = Date.now(),
): { thread: InboxThread; day: InboxDay | null }[] {
  let last: InboxDay | null = null;
  return threads.map((thread) => {
    const day = inboxDay(thread.latest.created_at_ms, now);
    const label = day === last ? null : day;
    last = day;
    return { thread, day: label };
  });
}

function projectFor(thread: InboxThread, projects: ProjectSnapshot[]): ProjectSnapshot | undefined {
  if (thread.projectId) return projects.find((p) => p.id === thread.projectId);
  const remote = thread.latest.target.remote?.toLowerCase();
  return remote ? projects.find((p) => p.remote?.toLowerCase() === remote) : undefined;
}

function projectPrs(p: ProjectSnapshot): PrInfo[] {
  return [...p.my_prs, ...p.prs_to_review, ...p.saved_prs, ...p.recent_prs, ...p.recently_merged];
}

/** The PR's own title when the sidebar lists it; otherwise the newest event's. */
export function inboxThreadTitle(thread: InboxThread, projects: ProjectSnapshot[]): string {
  const project = projectFor(thread, projects);
  const pr = project && thread.prNumber != null
    ? projectPrs(project).find((p) => p.number === thread.prNumber)
    : undefined;
  return pr?.title || thread.latest.title;
}

export function inboxThreadRepo(thread: InboxThread, projects: ProjectSnapshot[]): string {
  const project = projectFor(thread, projects);
  if (project) return project.name;
  const remote = thread.latest.target.remote ?? "";
  return remote.split("/").filter(Boolean).pop() ?? "";
}

export function inboxThreadRef(thread: InboxThread, projects: ProjectSnapshot[]): string {
  const repo = inboxThreadRepo(thread, projects);
  if (thread.prNumber != null) return `${repo} #${thread.prNumber}`.trim();
  return [repo, thread.latest.target.branch].filter(Boolean).join(" · ");
}

export type InboxRole = "author" | "reviewer";

/** From the sidebar's PR lists first, then from what the events imply. */
export function inboxThreadRole(thread: InboxThread, projects: ProjectSnapshot[]): InboxRole | null {
  const project = projectFor(thread, projects);
  const n = thread.prNumber;
  if (project && n != null) {
    if (project.my_prs.some((p) => p.number === n)) return "author";
    if (project.prs_to_review.some((p) => p.number === n)) return "reviewer";
  }
  if (thread.categories.includes("review_requested")) return "reviewer";
  if (thread.categories.some((c) => AUTHOR_CATEGORIES.has(c))) return "author";
  return null;
}

/** A tab already showing this thread's PR or branch: select it instead of opening another. */
export function findOpenTab(
  thread: InboxThread,
  tabs: TabSummary[],
  projects: ProjectSnapshot[],
): TabSummary | null {
  const project = projectFor(thread, projects);
  const remote = (thread.latest.target.remote ?? project?.remote ?? "").toLowerCase();
  const root = thread.latest.target.repo_root ?? project?.root_path ?? "";
  // A remote PR tab views a repo other than the clone it sits in, so the
  // remote decides whenever both sides know one.
  const sameRepo = (t: TabSummary) =>
    remote !== "" && t.remote
      ? t.remote.toLowerCase() === remote
      : root !== "" && t.repo_root === root;
  const branch = thread.latest.target.branch;
  return (
    tabs.find((t) => {
      if (!sameRepo(t)) return false;
      if (thread.prNumber != null) return t.pr_number === thread.prNumber;
      return branch != null && t.kind !== "remote_pr" && t.branch === branch;
    }) ?? null
  );
}

/** The thread to show: the chosen one while it is visible, else the first. */
export function selectedInboxThread(threads: InboxThread[], key: string | null): InboxThread | null {
  return threads.find((t) => t.key === key) ?? threads[0] ?? null;
}

/** j/k: the neighbour of the current row, clamped at both ends. */
export function stepInboxSelection(
  threads: InboxThread[],
  key: string | null,
  direction: 1 | -1,
): InboxThread | null {
  if (threads.length === 0) return null;
  const i = threads.findIndex((t) => t.key === key);
  if (i === -1) return threads[0];
  return threads[Math.max(0, Math.min(threads.length - 1, i + direction))];
}
