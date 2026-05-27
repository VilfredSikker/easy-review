import { threadAnchorEnd } from "$lib/diffAnnotations";
import type { ThreadSnapshot } from "$lib/types";

type ThreadLineRef = Pick<ThreadSnapshot, "line" | "line_end">;

/** "line 36" or "lines 36–41" — matches composer `rangeLabel()` copy. */
export function threadLineRangeLabel(thread: ThreadLineRef): string {
  if (thread.line <= 0) return "";
  const end = threadAnchorEnd(thread as ThreadSnapshot);
  return thread.line === end ? `line ${thread.line}` : `lines ${thread.line}–${end}`;
}

/** `path/file.rs:36` or `path/file.rs:36–41` */
export function threadFileLineRef(thread: Pick<ThreadSnapshot, "file" | "line" | "line_end">): string {
  if (thread.line <= 0) return thread.file;
  const end = threadAnchorEnd(thread);
  return thread.line === end
    ? `${thread.file}:${thread.line}`
    : `${thread.file}:${thread.line}–${end}`;
}

/** Line suffix only: `36` or `36–41` (for `basename(file):suffix`). */
export function threadLineRefSuffix(thread: ThreadLineRef): string {
  if (thread.line <= 0) return "";
  const end = threadAnchorEnd(thread as ThreadSnapshot);
  return thread.line === end ? `${thread.line}` : `${thread.line}–${end}`;
}
