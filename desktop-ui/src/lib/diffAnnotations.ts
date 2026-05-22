/** Pure annotation placement helpers for the diff view.
 *
 * Extracted from DiffView.svelte so the same placement logic can be reused
 * by the flat cross-file virtualizer (Step A0 of `.work/flat-virtualizer`).
 *
 * All exports are pure functions of their inputs. No Svelte state is captured.
 */

import type {
  AiSnapshot,
  FileSnapshot,
  FlatFinding,
  LineSnapshot,
  ThreadSnapshot,
} from "$lib/types";

export interface AnnotationIndex {
  /** `${file}:${line}` → findings anchored to that line. */
  findingsByFileLine: Map<string, FlatFinding[]>;
  /** `${file}` → findings without a line anchor. */
  findingsByFile: Map<string, FlatFinding[]>;
  /** finding id → finding. Fast lookup for flat model row dispatch. */
  findingMap: Map<string, FlatFinding>;
  /** thread id → thread snapshot. Includes both ai.threads and per-hunk threads. */
  threadMap: Map<string, ThreadSnapshot>;
  /** Thread IDs owned by a finding (rendered inside the finding card, not as standalone thread). */
  findingThreadIds: Set<string>;
  /** `${path}#${hunkIdx}` → ThreadSnapshot[] from `file.hunks[].threads`. */
  threadsByHunk: Map<string, ThreadSnapshot[]>;
  /** Stable hash; see annotationVersion. */
  version: number;
}

export interface CommentVisibility {
  hideAll: boolean;
  hideResolved: boolean;
  hideOutdated: boolean;
}

/** Minimal AiSnapshot subset used by the helpers. */
type AiInput = Pick<AiSnapshot, "threads" | "findings">;

function lineNum(line: LineSnapshot): number | null {
  return line.new_num ?? line.old_num;
}

/** Non-cryptographic deterministic string hash (djb2). */
function hashStr(s: string): number {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = ((h * 33) ^ s.charCodeAt(i)) | 0;
  }
  return h;
}

export function annotationVersion(
  ai: AiInput,
  files: FileSnapshot[],
  mode: string,
  vis: CommentVisibility,
): number {
  let h = 17;
  for (const t of ai.threads) h = (h * 31 + hashStr(t.id) + (t.resolved ? 1 : 0) + (t.stale ? 2 : 0)) | 0;
  for (const f of ai.findings) h = (h * 31 + hashStr(f.id) + (f.thread_id ? hashStr(f.thread_id) : 0)) | 0;
  h = (h * 31 + files.length) | 0;
  for (const file of files) {
    for (const hunk of file.hunks) {
      h = (h * 31 + hunk.threads.length) | 0;
      for (const t of hunk.threads) h = (h * 31 + hashStr(t.id) + (t.resolved ? 1 : 0)) | 0;
    }
  }
  h = (h * 31 + (vis.hideAll ? 1 : 0) + (vis.hideResolved ? 2 : 0) + (vis.hideOutdated ? 4 : 0)) | 0;
  h = (h * 31 + hashStr(mode)) | 0;
  return h;
}

export function buildAnnotationIndex(
  ai: AiInput,
  files: FileSnapshot[],
  mode: string,
  visibility: CommentVisibility,
): AnnotationIndex {
  const findingsByFileLine = new Map<string, FlatFinding[]>();
  const findingsByFile = new Map<string, FlatFinding[]>();
  for (const f of ai.findings) {
    if (f.line === null) {
      const bucket = findingsByFile.get(f.file);
      if (bucket) bucket.push(f);
      else findingsByFile.set(f.file, [f]);
    } else {
      const k = `${f.file}:${f.line}`;
      const bucket = findingsByFileLine.get(k);
      if (bucket) bucket.push(f);
      else findingsByFileLine.set(k, [f]);
    }
  }

  const threadMap = new Map<string, ThreadSnapshot>();
  for (const t of ai.threads) threadMap.set(t.id, t);

  const threadsByHunk = new Map<string, ThreadSnapshot[]>();
  for (const file of files) {
    for (let i = 0; i < file.hunks.length; i++) {
      const hunk = file.hunks[i];
      threadsByHunk.set(`${file.path}#${i}`, hunk.threads);
      for (const t of hunk.threads) {
        if (!threadMap.has(t.id)) threadMap.set(t.id, t);
      }
    }
  }

  const findingMap = new Map<string, FlatFinding>();
  const findingThreadIds = new Set<string>();
  for (const f of ai.findings) {
    findingMap.set(f.id, f);
    if (f.thread_id) findingThreadIds.add(f.thread_id);
  }

  return {
    findingsByFileLine,
    findingsByFile,
    findingMap,
    threadMap,
    findingThreadIds,
    threadsByHunk,
    version: annotationVersion(ai, files, mode, visibility),
  };
}

/** Branch mode: match hunk_index when set. Other modes: line number only. */
function findingMatchesHunk(f: FlatFinding, hunkIndex: number, mode: string): boolean {
  if (mode !== "branch") return true;
  return f.hunk_index === null || f.hunk_index === hunkIndex;
}

export function findingBelongsToHunk(
  f: FlatFinding,
  filePath: string,
  hunkIndex: number,
  hunk: { new_start: number; new_count: number },
  mode: string,
): boolean {
  if (f.file !== filePath) return false;
  if (f.hunk_index !== null) return f.hunk_index === hunkIndex;
  if (f.line !== null) {
    return f.line >= hunk.new_start && f.line < hunk.new_start + hunk.new_count;
  }
  return mode === "branch" ? false : f.hunk_index === hunkIndex;
}

export function findingsForLine(
  idx: AnnotationIndex,
  filePath: string,
  hunkIndex: number,
  targetLine: number,
  hunkLines: LineSnapshot[],
  skipDelDuplicate: boolean,
  mode: string,
): FlatFinding[] {
  const candidates = idx.findingsByFileLine.get(`${filePath}:${targetLine}`) ?? [];
  return candidates.filter((f) => {
    if (!findingMatchesHunk(f, hunkIndex, mode)) return false;
    if (skipDelDuplicate && hunkLines.some((l) => l.new_num === targetLine)) return false;
    return true;
  });
}

export function findingRendersInline(
  f: FlatFinding,
  filePath: string,
  hunkIndex: number,
  hunkLines: LineSnapshot[],
  mode: string,
): boolean {
  if (f.file !== filePath || f.line === null) return false;
  if (!findingMatchesHunk(f, hunkIndex, mode)) return false;
  for (const line of hunkLines) {
    const ln = lineNum(line);
    if (ln !== f.line) continue;
    if (line.kind === "del" && hunkLines.some((l) => l.new_num === f.line)) continue;
    return true;
  }
  return false;
}

export function hunkLevelFindings(
  idx: AnnotationIndex,
  filePath: string,
  hunkIndex: number,
  hunk: { new_start: number; new_count: number },
  mode: string,
): FlatFinding[] {
  const candidates = idx.findingsByFile.get(filePath) ?? [];
  return candidates.filter((f) => findingBelongsToHunk(f, filePath, hunkIndex, hunk, mode));
}

export function fallbackFindings(
  idx: AnnotationIndex,
  filePath: string,
  hunkIndex: number,
  hunk: { new_start: number; new_count: number },
  hunkLines: LineSnapshot[],
  mode: string,
): FlatFinding[] {
  const lo = hunk.new_start;
  const hi = hunk.new_start + hunk.new_count;
  const out: FlatFinding[] = [];
  for (let ln = lo; ln < hi; ln++) {
    const candidates = idx.findingsByFileLine.get(`${filePath}:${ln}`) ?? [];
    for (const f of candidates) {
      if (findingBelongsToHunk(f, filePath, hunkIndex, hunk, mode) &&
          !findingRendersInline(f, filePath, hunkIndex, hunkLines, mode)) {
        out.push(f);
      }
    }
  }
  return out;
}

export function findingsForSplitRow(
  idx: AnnotationIndex,
  filePath: string,
  hunkIndex: number,
  leftLn: number | null,
  rightLn: number | null,
  hunkLines: LineSnapshot[],
  mode: string,
): FlatFinding[] {
  const out: FlatFinding[] = [];
  const seen = new Set<string>();
  for (const ln of [rightLn, leftLn]) {
    if (ln === null) continue;
    for (const f of findingsForLine(idx, filePath, hunkIndex, ln, hunkLines, false, mode)) {
      if (seen.has(f.id)) continue;
      seen.add(f.id);
      out.push(f);
    }
  }
  return out;
}

/** Apply CommentVisibility + findingThreadIds gates to a per-hunk thread list. */
function visibleThreads(
  threads: ThreadSnapshot[],
  findingThreadIds: Set<string>,
  vis: CommentVisibility,
): ThreadSnapshot[] {
  if (vis.hideAll) return [];
  return threads.filter(
    (t) =>
      !findingThreadIds.has(t.id) &&
      !(vis.hideResolved && t.resolved) &&
      !(vis.hideOutdated && t.stale),
  );
}

export function threadsForLine(
  idx: AnnotationIndex,
  filePath: string,
  hunkIndex: number,
  line: number,
  _hunkLines: LineSnapshot[],
  vis: CommentVisibility = { hideAll: false, hideResolved: false, hideOutdated: false },
): ThreadSnapshot[] {
  const threads = idx.threadsByHunk.get(`${filePath}#${hunkIndex}`) ?? [];
  return visibleThreads(threads, idx.findingThreadIds, vis).filter((t) => t.line === line);
}

export function fallbackThreadsForHunk(
  idx: AnnotationIndex,
  filePath: string,
  hunkIndex: number,
  _hunk: { new_start: number; new_count: number },
  renderedLineNums: Set<number>,
  vis: CommentVisibility = { hideAll: false, hideResolved: false, hideOutdated: false },
): ThreadSnapshot[] {
  const threads = idx.threadsByHunk.get(`${filePath}#${hunkIndex}`) ?? [];
  return visibleThreads(threads, idx.findingThreadIds, vis).filter(
    (t) => !renderedLineNums.has(t.line),
  );
}

/** Exposed for callers that need the visibility filter against a raw thread list
 *  (e.g. DiffView's current template that filters `hunk.threads` directly). */
export function applyThreadVisibility(
  threads: ThreadSnapshot[],
  findingThreadIds: Set<string>,
  vis: CommentVisibility,
): ThreadSnapshot[] {
  return visibleThreads(threads, findingThreadIds, vis);
}
