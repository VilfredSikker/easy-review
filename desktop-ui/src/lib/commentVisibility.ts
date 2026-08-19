import type { ThreadSnapshot } from "./types";

export interface CommentVisibility {
  hideAll: boolean;
  hideResolved: boolean;
  hideOutdated: boolean;
  /** Hide GitHub comment threads (kind === "comment"). */
  hideComments: boolean;
  /** Hide AI findings. */
  hideFindings: boolean;
  /** Hide personal question threads (kind === "question"). */
  hideQuestions: boolean;
}

/** App default: hide resolved + outdated in the Branch panel / badge. */
export const DEFAULT_COMMENT_VISIBILITY: CommentVisibility = {
  hideAll: false,
  hideResolved: true,
  hideOutdated: true,
  hideComments: false,
  hideFindings: false,
  hideQuestions: false,
};

/** All annotations visible — used as a fallback in diff annotation helpers. */
export const ALL_VISIBLE: CommentVisibility = {
  hideAll: false,
  hideResolved: false,
  hideOutdated: false,
  hideComments: false,
  hideFindings: false,
  hideQuestions: false,
};

/** Top-level GitHub comment threads (Branch tab badge / CommentsCard). */
export function commentThreads(threads: ThreadSnapshot[] | undefined | null): ThreadSnapshot[] {
  return threads?.filter((t) => t.kind === "comment") ?? [];
}

type DiffFiles = { hunks: { threads: ThreadSnapshot[] }[] }[] | null | undefined;

/**
 * GitHub comment threads for the Comments panel / Branch badge.
 *
 * Inline diff rows render `file.hunks[].threads`. The panel used to read only
 * `ai.threads`. Those copies can disagree (differential snapshots splice old
 * hunks while replacing AI), which shows comments in the diff and hides them
 * in the panel. Prefer the hunk copy when both exist so the panel matches
 * what the user is scrolling.
 */
export function commentThreadsFromDiff(
  aiThreads: ThreadSnapshot[] | undefined | null,
  files?: DiffFiles,
): ThreadSnapshot[] {
  const hunkById = new Map<string, ThreadSnapshot>();
  for (const file of files ?? []) {
    for (const hunk of file.hunks ?? []) {
      for (const t of hunk.threads ?? []) {
        if (t.kind === "comment") hunkById.set(t.id, t);
      }
    }
  }
  const seen = new Set<string>();
  const out: ThreadSnapshot[] = [];
  for (const t of commentThreads(aiThreads)) {
    seen.add(t.id);
    out.push(hunkById.get(t.id) ?? t);
  }
  for (const [id, t] of hunkById) {
    if (!seen.has(id)) out.push(t);
  }
  return out;
}

/**
 * Local comments that have not been pushed to GitHub.
 *
 * Review submit and individual push only send these. Existing GitHub
 * threads and already-synced local comments are not included.
 */
export function unpushedLocalCommentThreads(
  threads: ThreadSnapshot[] | undefined | null,
  files?: DiffFiles,
): ThreadSnapshot[] {
  return commentThreadsFromDiff(threads, files).filter(
    (t) => t.source === "local" && !t.synced,
  );
}

/**
 * Comment threads visible under the current hide-resolved / hide-outdated /
 * hide-all filters. Branch badge and CommentsCard must use the same set.
 */
export function visibleCommentThreads(
  threads: ThreadSnapshot[] | undefined | null,
  visibility: CommentVisibility,
  files?: DiffFiles,
): ThreadSnapshot[] {
  const all = commentThreadsFromDiff(threads, files);
  if (visibility.hideAll) return [];
  return all.filter(
    (thread) =>
      !(visibility.hideResolved && thread.resolved) &&
      !(visibility.hideOutdated && thread.stale),
  );
}

/** Toggle one Comments-panel filter. Clearing Hide all when the user
 *  flips Hide outdated / Hide resolved, otherwise those clicks are no-ops. */
export function toggleCommentFilter(
  current: CommentVisibility,
  key: "hideOutdated" | "hideResolved" | "hideAll",
): CommentVisibility {
  if (key === "hideAll") {
    return { ...current, hideAll: !current.hideAll };
  }
  return { ...current, [key]: !current[key], hideAll: false };
}

/** Empty-state copy for the Comments panel. Distinguishes Hide all from
 *  the resolved/outdated filters so the user is told the filter that
 *  actually emptied the list. */
export function hiddenCommentsHint(hiddenCount: number, visibility: CommentVisibility): string {
  const n = `${hiddenCount} comment${hiddenCount === 1 ? "" : "s"} hidden`;
  if (visibility.hideAll) {
    return `${n}. Turn off Hide all above to show them.`;
  }
  return `${n} (resolved/outdated). Turn off Hide resolved or Hide outdated above to show them.`;
}
