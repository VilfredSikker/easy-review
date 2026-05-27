import { threadAnchorEnd } from "$lib/diffAnnotations";
import type { ThreadSnapshot } from "$lib/types";

type ThreadLineRef = Pick<ThreadSnapshot, "line" | "line_end">;

/** Line suffix only: `36` or `36–41` (for `basename(file):suffix`). */
export function threadLineRefSuffix(thread: ThreadLineRef): string {
  if (thread.line <= 0) return "";
  const end = threadAnchorEnd(thread as ThreadSnapshot);
  return thread.line === end ? `${thread.line}` : `${thread.line}–${end}`;
}
