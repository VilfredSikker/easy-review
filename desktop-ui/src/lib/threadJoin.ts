import type { Hunk } from "$lib/protocol/generated/Hunk";
import type { HunkSnapshot, ThreadSnapshot } from "$lib/types";

const HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

/** The ranges a hunk's header spans: folds hide context, so a thread past a fold still sits inside them. */
function span(hunk: Hunk): { oldStart: number; oldCount: number; newStart: number; newCount: number } {
  const m = HEADER.exec(hunk.header);
  if (!m) return { oldStart: hunk.old_start, oldCount: hunk.old_count, newStart: hunk.new_start, newCount: hunk.new_count };
  return { oldStart: Number(m[1]), oldCount: m[2] === undefined ? 1 : Number(m[2]), newStart: Number(m[3]), newCount: m[4] === undefined ? 1 : Number(m[4]) };
}

/**
 * Whether `thread` sits on the hunk at `index`, by the engine's rule
 * (`comments_for_hunk_or_line_range`): a thread with no line names its hunk;
 * a LEFT thread's old-side line falls in the hunk's old range; any other line
 * is a new-side line and falls in the new range only, even on a delete-only
 * hunk, whose old numbers can repeat an earlier hunk's new ones.
 */
export function threadOnHunk(thread: ThreadSnapshot, hunk: Hunk, index: number): boolean {
  const line = thread.line > 0 ? thread.line : null;
  const old = thread.old_line ?? null;
  if (line === null && old === null) return thread.hunk_index === index;
  const s = span(hunk);
  if (thread.side?.toUpperCase() === "LEFT") {
    const n = old ?? line;
    return n !== null && n >= s.oldStart && n < s.oldStart + s.oldCount;
  }
  return line !== null && line >= s.newStart && line < s.newStart + s.newCount;
}

/** `hunks` with the threads that sit on each, in the order the annotations list them. */
export function joinThreads(hunks: readonly Hunk[], threads: readonly ThreadSnapshot[]): HunkSnapshot[] {
  return hunks.map((hunk, index) => ({
    ...hunk,
    threads: threads
      .filter((thread) => threadOnHunk(thread, hunk, index))
      .map((thread) => (thread.side ? thread : { ...thread, side: "RIGHT" })),
  }));
}

const NONE: readonly ThreadSnapshot[] = [];
const byFile = new WeakMap<readonly ThreadSnapshot[], Map<string, ThreadSnapshot[]>>();
const joined = new WeakMap<readonly Hunk[], { threads: readonly ThreadSnapshot[]; hunks: HunkSnapshot[] }>();

/** The threads on `path`, grouped once per thread list. */
export function threadsOn(threads: readonly ThreadSnapshot[], path: string): readonly ThreadSnapshot[] {
  let groups = byFile.get(threads);
  if (!groups) {
    groups = new Map();
    for (const thread of threads) {
      const group = groups.get(thread.file);
      if (group) group.push(thread);
      else groups.set(thread.file, [thread]);
    }
    byFile.set(threads, groups);
  }
  return groups.get(path) ?? NONE;
}

/** `joinThreads`, kept while the hunks and the file's threads are the same arrays, so an unchanged file keeps its rows. */
export function joinedHunks(hunks: readonly Hunk[], threads: readonly ThreadSnapshot[]): HunkSnapshot[] {
  const held = joined.get(hunks);
  if (held && held.threads === threads) return held.hunks;
  const next = joinThreads(hunks, threads);
  joined.set(hunks, { threads, hunks: next });
  return next;
}
