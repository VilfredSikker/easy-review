import type { AppSnapshot, FileSnapshot, ThreadSnapshot } from "./types";

/**
 * Whether a question or note is anchored on a document line outside the diff
 * (ADR 0041), which GitHub will not take as a comment. The snapshot carries no
 * flag for it, but the engine places every hunk-anchored thread in its hunk's
 * `threads`, so a line-anchored thread that no hunk holds is a document anchor.
 *
 * Answers false whenever the file's hunks are not all in hand (a lazy stub, a
 * compacted file, an omitted delta, or a file outside the snapshot): offering
 * Promote and letting the engine refuse it beats hiding a valid action.
 */
export function isDocumentAnchored(
  thread: Pick<ThreadSnapshot, "id" | "kind" | "file" | "line">,
  files: readonly FileSnapshot[] | undefined,
): boolean {
  if (thread.kind === "comment" || thread.line <= 0) return false;
  const file = files?.find((f) => f.path === thread.file);
  if (!file || file.is_lazy_stub || file.compacted || file.hunks_omitted) return false;
  return !file.hunks.some((h) => h.threads.some((t) => t.id === thread.id));
}

function findThread(snapshot: AppSnapshot, id: string): ThreadSnapshot | undefined {
  const flat = snapshot.ai.threads.find((t) => t.id === id);
  if (flat) return flat;
  for (const file of snapshot.files) {
    for (const hunk of file.hunks) {
      const held = hunk.threads.find((t) => t.id === id);
      if (held) return held;
    }
  }
  return undefined;
}

/**
 * How many of the reviewer's own replies on `threadId` read `text`. Compared
 * before and after a reply is awaited, it tells whether the reply landed: a
 * failed or skipped optimistic write is rolled back without throwing.
 */
export function ownReplyCount(snapshot: AppSnapshot | null, threadId: string, text: string): number {
  if (!snapshot) return 0;
  const thread = findThread(snapshot, threadId);
  if (!thread) return 0;
  return thread.replies.filter((r) => r.kind === "you" && r.body_markdown.trim() === text).length;
}
