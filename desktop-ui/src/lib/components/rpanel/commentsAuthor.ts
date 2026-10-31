import type { ThreadSnapshot } from "$lib/types";
import { sameAuthor } from "$lib/reviewerComments";

/**
 * The GitHub threads one reviewer took part in, by root or reply. The reviewer
 * popover counts the same things, so "Show in Comments panel" lists every
 * thread its inline entries came from. Questions and notes never left the
 * machine, and an AI reply is not the reviewer's, so neither matches.
 */
export function threadsByAuthor<T extends Pick<ThreadSnapshot, "kind" | "root" | "replies">>(
  threads: readonly T[],
  author: string | null,
): T[] {
  if (!author) return [...threads];
  return threads.filter(
    (t) =>
      t.kind === "comment" &&
      (sameAuthor(t.root.author, author) ||
        t.replies.some((r) => r.kind !== "ai" && sameAuthor(r.author, author))),
  );
}
