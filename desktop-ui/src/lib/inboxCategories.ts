import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";

export const INBOX_CATEGORY_ORDER = [
  "pr_comment",
  "review_received",
  "comment_reply",
  "approved",
  "changes_requested",
  "review_requested",
  "mention",
  "ci",
  "lifecycle",
  "ai",
  "other",
] as const;

export type InboxCategoryId = (typeof INBOX_CATEGORY_ORDER)[number];

export const INBOX_CATEGORY_LABELS: Record<InboxCategoryId, string> = {
  pr_comment: "Comment on your PR",
  review_received: "Review received",
  comment_reply: "Reply on comment",
  approved: "Approved",
  changes_requested: "Changes requested",
  review_requested: "Review requested",
  mention: "Mention",
  ci: "CI",
  lifecycle: "Merged / closed",
  ai: "AI",
  other: "Other",
};

const KIND_TO_CATEGORY: Record<string, InboxCategoryId> = {
  pr_comment: "pr_comment",
  new_comment: "pr_comment",
  comment: "pr_comment",
  pr_review_received: "review_received",
  pr_comment_reply: "comment_reply",
  pr_review_approved: "approved",
  pr_review_changes_requested: "changes_requested",
  review_requested: "review_requested",
  review_rerequested: "review_requested",
  review: "review_requested",
  mention: "mention",
  ci_failed: "ci",
  "ci-fail": "ci",
  check_failed: "ci",
  pr_merged: "lifecycle",
  pr_closed: "lifecycle",
  merged: "lifecycle",
  ai_review_done: "ai",
  ai_review_failed: "ai",
  ai_triage_done: "ai",
  ai_triage_failed: "ai",
  ai_review_cancelled: "ai",
};

export function inboxItemCategory(item: InboxItemSnapshot): InboxCategoryId {
  const raw = item.category;
  if (raw && (INBOX_CATEGORY_ORDER as readonly string[]).includes(raw)) {
    return raw as InboxCategoryId;
  }
  return KIND_TO_CATEGORY[item.kind] ?? "other";
}

export function inboxCategoryLabel(category: string): string {
  if ((INBOX_CATEGORY_ORDER as readonly string[]).includes(category)) {
    return INBOX_CATEGORY_LABELS[category as InboxCategoryId];
  }
  return INBOX_CATEGORY_LABELS.other;
}

export interface InboxCategoryGroup {
  category: InboxCategoryId;
  label: string;
  items: InboxItemSnapshot[];
}

export function groupInboxItems(items: InboxItemSnapshot[]): InboxCategoryGroup[] {
  const buckets = new Map<InboxCategoryId, InboxItemSnapshot[]>();
  for (const item of items) {
    const category = inboxItemCategory(item);
    const list = buckets.get(category);
    if (list) list.push(item);
    else buckets.set(category, [item]);
  }
  const groups: InboxCategoryGroup[] = [];
  for (const category of INBOX_CATEGORY_ORDER) {
    const grouped = buckets.get(category);
    if (!grouped?.length) continue;
    groups.push({
      category,
      label: INBOX_CATEGORY_LABELS[category],
      items: grouped,
    });
  }
  return groups;
}

export interface InboxCategoryChip {
  category: InboxCategoryId;
  label: string;
  total: number;
  unread: number;
}

export function inboxCategoryChips(items: InboxItemSnapshot[]): InboxCategoryChip[] {
  const counts = new Map<InboxCategoryId, { total: number; unread: number }>();
  for (const item of items) {
    const category = inboxItemCategory(item);
    const entry = counts.get(category) ?? { total: 0, unread: 0 };
    entry.total += 1;
    if (item.read_at_ms == null) entry.unread += 1;
    counts.set(category, entry);
  }
  return INBOX_CATEGORY_ORDER.flatMap((category) => {
    const entry = counts.get(category);
    if (!entry) return [];
    return [
      {
        category,
        label: INBOX_CATEGORY_LABELS[category],
        total: entry.total,
        unread: entry.unread,
      },
    ];
  });
}

export function sortInboxItems(items: InboxItemSnapshot[]): InboxItemSnapshot[] {
  return [...items].sort((a, b) => {
    const aUnread = a.read_at_ms == null ? 0 : 1;
    const bUnread = b.read_at_ms == null ? 0 : 1;
    if (aUnread !== bUnread) return aUnread - bUnread;
    return b.created_at_ms - a.created_at_ms;
  });
}

export const INBOX_POPOVER_LIMIT = 20;

export function applyInboxFilters(
  items: InboxItemSnapshot[],
  opts: {
    projects: ProjectSnapshot[];
    projectId: "all" | string;
    read: "all" | "unread" | "read";
    category: "all" | InboxCategoryId;
  },
): InboxItemSnapshot[] {
  return items.filter((item) => {
    if (
      opts.projectId !== "all" &&
      inboxItemProjectId(item, opts.projects) !== opts.projectId
    ) {
      return false;
    }
    if (opts.read === "unread" && item.read_at_ms != null) return false;
    if (opts.read === "read" && item.read_at_ms == null) return false;
    if (opts.category !== "all" && inboxItemCategory(item) !== opts.category) {
      return false;
    }
    return true;
  });
}

export function inboxItemProjectId(
  item: InboxItemSnapshot,
  projects: ProjectSnapshot[],
): string | null {
  if (item.target.project_id) return item.target.project_id;
  const root = item.target.repo_root;
  if (root) {
    const match = projects.find((p) => p.root_path && p.root_path === root);
    if (match) return match.id;
  }
  const remote = item.target.remote;
  if (remote) {
    const match = projects.find((p) => p.remote && p.remote === remote);
    if (match) return match.id;
  }
  return null;
}

export function formatInboxAge(createdAtMs: number, now = Date.now()): string {
  const delta = now - createdAtMs;
  if (delta < 60_000) return "now";
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m`;
  return `${Math.floor(delta / 3_600_000)}h`;
}

export function formatInboxUpdated(ms: number, now = Date.now()): string {
  if (!ms || ms <= 0) return "never";
  const delta = now - ms;
  if (delta < 60_000) return "just now";
  const mins = Math.floor(delta / 60_000);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ago`;
}

export interface InboxKindMeta {
  color: string;
  path: string;
}

const KIND_META: Record<string, InboxKindMeta> = {
  pr_merged: {
    color: "text-periwinkle",
    path: "M18 6 6 18M6 6l12 12M6 3v6h6",
  },
  merged: {
    color: "text-periwinkle",
    path: "M18 6 6 18M6 6l12 12M6 3v6h6",
  },
  pr_closed: {
    color: "text-muted",
    path: "M18 6 6 18M6 6l12 12",
  },
  ci_failed: {
    color: "text-del-fg",
    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6",
  },
  "ci-fail": {
    color: "text-del-fg",
    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6",
  },
  check_failed: {
    color: "text-del-fg",
    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6",
  },
  review_requested: {
    color: "text-accent",
    path: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  },
  review_rerequested: {
    color: "text-accent",
    path: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  },
  review: {
    color: "text-accent",
    path: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  },
  pr_review_received: {
    color: "text-accent",
    path: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
  },
  pr_review_approved: {
    color: "text-add-fg",
    path: "M22 11.08V12a10 10 0 1 1-5.93-9.14 M22 4 12 14.01l-3-3",
  },
  pr_review_changes_requested: {
    color: "text-warning",
    path: "M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0zM12 9v4M12 17h.01",
  },
  pr_comment: {
    color: "text-comment",
    path: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
  },
  pr_comment_reply: {
    color: "text-comment",
    path: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z M13 8H8 M16 12H8",
  },
  new_comment: {
    color: "text-comment",
    path: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
  },
  comment: {
    color: "text-comment",
    path: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z",
  },
  mention: {
    color: "text-warning",
    path: "M16 8a6 6 0 0 1-12 0 6 6 0 0 1 12 0zM16 8c0 3.3 1.7 6 4 6M20 8v4M20 8a8 8 0 1 0-8 8",
  },
  ai_review_done: {
    color: "text-add-fg",
    path: "M12 2l2.4 7.2H22l-6 4.6 2.3 7.2L12 16.4 5.7 21l2.3-7.2-6-4.6h7.6z",
  },
  ai_triage_done: {
    color: "text-add-fg",
    path: "M12 2l2.4 7.2H22l-6 4.6 2.3 7.2L12 16.4 5.7 21l2.3-7.2-6-4.6h7.6z",
  },
  ai_review_failed: {
    color: "text-del-fg",
    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6",
  },
  ai_triage_failed: {
    color: "text-del-fg",
    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6",
  },
  github_refresh_failed: {
    color: "text-warning",
    path: "M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0zM12 9v4M12 17h.01",
  },
};

const SEVERITY_META: Record<string, InboxKindMeta> = {
  error: {
    color: "text-del-fg",
    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6",
  },
  warning: {
    color: "text-warning",
    path: "M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0zM12 9v4M12 17h.01",
  },
};

const DEFAULT_META: InboxKindMeta = {
  color: "text-muted",
  path: "M18 8h1a4 4 0 0 1 0 8h-1M2 8h16v9a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V8zM6 1v3M10 1v3M14 1v3",
};

export function inboxKindMeta(item: InboxItemSnapshot): InboxKindMeta {
  return KIND_META[item.kind] ?? SEVERITY_META[item.severity] ?? DEFAULT_META;
}
