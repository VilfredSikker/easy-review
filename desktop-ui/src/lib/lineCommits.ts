import type { LineCommits } from "$lib/protocol/generated/LineCommits";
import { lineCommits } from "$lib/tabCommands";

/** Reply of the lazy `line_commits` command. It is not snapshot data: ask for
 *  it per file when "Mark lines" is on and the file is visible. */
export type { LineCommits };

export function fetchLineCommits(path: string, cacheKey: string): Promise<LineCommits> {
  return lineCommits(path, cacheKey);
}

/** A reply is usable only for the hunks it was computed against. */
export function lineCommitsCurrent(result: LineCommits, path: string, cacheKey: string): boolean {
  return result.path === path && result.cache_key === cacheKey;
}

/** The sha that introduced (`add`) or removed (`del`) a line, or null when the
 *  line predates the range, is uncommitted, or was not attributed. Spans come
 *  sorted by line, so this is a binary search. */
export function lineCommitSha(result: LineCommits, side: "add" | "del", line: number): string | null {
  const spans = side === "add" ? result.added : result.deleted;
  let lo = 0;
  let hi = spans.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const span = spans[mid];
    if (line < span.start) hi = mid - 1;
    else if (line >= span.start + span.len) lo = mid + 1;
    else return result.commits[span.commit] ?? null;
  }
  return null;
}
