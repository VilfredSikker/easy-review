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

/**
 * Comment threads visible under the current hide-resolved / hide-outdated /
 * hide-all filters. Branch badge and CommentsCard must use the same set.
 */
export function visibleCommentThreads(
  threads: ThreadSnapshot[] | undefined | null,
  visibility: CommentVisibility,
): ThreadSnapshot[] {
  const all = commentThreads(threads);
  if (visibility.hideAll) return [];
  return all.filter(
    (thread) =>
      !(visibility.hideResolved && thread.resolved) &&
      !(visibility.hideOutdated && thread.stale),
  );
}
