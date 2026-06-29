import { splitRows } from "$lib/splitRows";
import type { FileSnapshot, FlatFinding, HunkSnapshot, LineSnapshot, ThreadSnapshot } from "$lib/types";
import type { SplitRow } from "$lib/splitRows";
import {
  fallbackFindings,
  fallbackThreadsForHunk,
  findingsForLine,
  findingsForSplitRow,
  hunkLevelFindings,
  threadsForLine,
  type AnnotationIndex,
  type CommentVisibility,
} from "$lib/diffAnnotations";

/**
 * For each line in a unified-diff hunk, holds the partner LineSnapshot for
 * word-diff pairing. For a `del` line paired with an `add`, `partner` is the
 * corresponding `add` line (and vice versa). For context, fold, and unpaired
 * lines, `partner` is null.
 */
export interface UnifiedPair {
  partner: LineSnapshot | null;
}

// ---------------- Legacy per-file model (kept for back-compat; removed in Step H) ----------------

export type FlatRow =
  | { type: "hunk-header"; hunkIdx: number; height: number }
  | { type: "content"; hunkIdx: number; rowIdx: number; height: number };

export interface FileRenderModel {
  cacheKey: string;
  unifiedPairsByHunk: UnifiedPair[][];
  splitRowsByHunk: SplitRow[][];
  flatRows: FlatRow[];
  cumulativeOffsets: number[];
  totalHeight: number;
  hunkContentStartOffsets: number[];
  splitHunkContentStartOffsets: number[];
}

export const LINE_HEIGHT = 24;
export const HUNK_HEADER_HEIGHT = 22;
export const FILE_HEADER_HEIGHT = 40;
export const COMPACTED_STUB_HEIGHT = 44;
export const NO_CHANGES_HEIGHT = 44;

const LEGACY_CACHE_LIMIT = 100;
const _legacyCache = new Map<string, FileRenderModel>();

export function computeUnifiedPairs(hunk: HunkSnapshot): UnifiedPair[] {
  const lines = hunk.lines;
  const pairs: UnifiedPair[] = new Array(lines.length).fill(null).map(() => ({ partner: null }));
  let i = 0;
  while (i < lines.length) {
    if (lines[i].kind !== "del") {
      i++;
      continue;
    }
    const delStart = i;
    while (i < lines.length && lines[i].kind === "del") i++;
    const addStart = i;
    while (i < lines.length && lines[i].kind === "add") i++;
    const delCount = addStart - delStart;
    const addCount = i - addStart;
    const pairCount = Math.min(delCount, addCount);
    for (let k = 0; k < pairCount; k++) {
      pairs[delStart + k] = { partner: lines[addStart + k] };
      pairs[addStart + k] = { partner: lines[delStart + k] };
    }
  }
  return pairs;
}

export function getFileRenderModel(file: FileSnapshot): FileRenderModel {
  const key = `${file.path}::${file.cache_key}`;
  const cached = _legacyCache.get(key);
  if (cached) return cached;

  const unifiedPairsByHunk: UnifiedPair[][] = file.hunks.map(computeUnifiedPairs);
  const splitRowsByHunk: SplitRow[][] = file.hunks.map((h) => splitRows(h.lines));

  const flatRows: FlatRow[] = [];
  const heights: number[] = [];

  for (let hunkIdx = 0; hunkIdx < file.hunks.length; hunkIdx++) {
    flatRows.push({ type: "hunk-header", hunkIdx, height: HUNK_HEADER_HEIGHT });
    heights.push(HUNK_HEADER_HEIGHT);
    const pairs = unifiedPairsByHunk[hunkIdx];
    for (let rowIdx = 0; rowIdx < pairs.length; rowIdx++) {
      flatRows.push({ type: "content", hunkIdx, rowIdx, height: LINE_HEIGHT });
      heights.push(LINE_HEIGHT);
    }
  }

  const cumulativeOffsets = new Array<number>(heights.length + 1);
  cumulativeOffsets[0] = 0;
  for (let i = 0; i < heights.length; i++) {
    cumulativeOffsets[i + 1] = cumulativeOffsets[i] + heights[i];
  }
  const totalHeight = cumulativeOffsets[heights.length];

  const hunkContentStartOffsets: number[] = [];
  let unifiedOffset = 0;
  for (let i = 0; i < file.hunks.length; i++) {
    unifiedOffset += HUNK_HEADER_HEIGHT;
    hunkContentStartOffsets.push(unifiedOffset);
    unifiedOffset += unifiedPairsByHunk[i].length * LINE_HEIGHT;
  }

  const splitHunkContentStartOffsets: number[] = [];
  let splitOffset = 0;
  for (let i = 0; i < file.hunks.length; i++) {
    splitOffset += HUNK_HEADER_HEIGHT;
    splitHunkContentStartOffsets.push(splitOffset);
    splitOffset += splitRowsByHunk[i].length * LINE_HEIGHT;
  }

  if (_legacyCache.size >= LEGACY_CACHE_LIMIT) {
    const firstKey = _legacyCache.keys().next().value;
    if (firstKey !== undefined) _legacyCache.delete(firstKey);
  }
  const model: FileRenderModel = {
    cacheKey: file.cache_key,
    unifiedPairsByHunk,
    splitRowsByHunk,
    flatRows,
    cumulativeOffsets,
    totalHeight,
    hunkContentStartOffsets,
    splitHunkContentStartOffsets,
  };
  _legacyCache.set(key, model);
  return model;
}

// ---------------- Step A: Flat cross-file row block ----------------

/** Data for a Guide pillar group (drives the left rail lane in Split View). */
export interface PillarHeaderInfo {
  pillarId: string;
  title: string;
  descriptionMarkdown: string;
  reviewedCount: number;
  totalCount: number;
  foundation: boolean;
}

export type CrossFileFlatRow =
  | {
      type: "file-header";
      filePath: string;
      fileIndex: number;
      sourceIndex: number;
      height: number;
      identity: string;
      additions: number;
      deletions: number;
    }
  | {
      type: "hunk-header";
      filePath: string;
      hunkIdx: number;
      header: string;
      height: number;
      identity: string;
    }
  | {
      type: "content-unified";
      filePath: string;
      hunkIdx: number;
      lineIdx: number;
      height: number;
      identity: string;
    }
  | {
      type: "content-fold";
      filePath: string;
      hunkIdx: number;
      lineIdx: number;
      label: string;
      height: number;
      identity: string;
    }
  | {
      type: "content-split";
      filePath: string;
      hunkIdx: number;
      splitRowIdx: number;
      height: number;
      identity: string;
    }
  | {
      type: "compacted-stub";
      filePath: string;
      fileIndex: number;
      sourceIndex: number;
      height: number;
      identity: string;
    }
  | {
      type: "lazy-stub";
      filePath: string;
      fileIndex: number;
      sourceIndex: number;
      height: number;
      identity: string;
    }
  | {
      type: "no-changes";
      filePath: string;
      fileIndex: number;
      sourceIndex: number;
      height: number;
      identity: string;
      /** Pure rename (no content change) — render "File renamed without changes." */
      renamed: boolean;
    }
  | {
      type: "inline-thread";
      filePath: string;
      hunkIdx: number;
      threadId: string;
      side: "unified" | "split";
      height: number;
      identity: string;
    }
  | {
      type: "inline-finding";
      filePath: string;
      hunkIdx: number;
      findingId: string;
      side: "unified" | "split";
      height: number;
      identity: string;
    }
  | {
      type: "fallback-thread";
      filePath: string;
      hunkIdx: number;
      threadId: string;
      side: "unified" | "split";
      height: number;
      identity: string;
    }
  | {
      type: "fallback-finding";
      filePath: string;
      hunkIdx: number;
      findingId: string;
      side: "unified" | "split";
      height: number;
      identity: string;
    };

export interface FileBlock {
  filePath: string;
  fileIndex: number;
  modelKey: string;
  rows: CrossFileFlatRow[];
  cumulativeOffsets: number[];
  totalHeight: number;
  unifiedPairsByHunk: UnifiedPair[][];
  splitRowsByHunk: SplitRow[][];
}

export interface RenderModelInputs {
  file: FileSnapshot;
  fileIndex: number;
  viewMode: "unified" | "split";
  mode: string;
  annotationIndex: AnnotationIndex;
  commentVisibility: CommentVisibility;
}

export function estimateLazyStubHeight(file: FileSnapshot): number {
  if (file.hunks.length === 0) {
    const lineCount = (file.additions ?? 0) + (file.deletions ?? 0);
    if (lineCount === 0) return 60;
    return HUNK_HEADER_HEIGHT + lineCount * LINE_HEIGHT;
  }
  let total = 0;
  for (const hunk of file.hunks) {
    total += HUNK_HEADER_HEIGHT + hunk.lines.length * LINE_HEIGHT;
  }
  return total;
}

/** Max scroll height for annotation body text (matches `.annotation-body-scroll`). */
const ANNOTATION_BODY_MAX_PX = 192;

export function estimateThreadHeight(thread: ThreadSnapshot): number {
  let h = 48; // header
  const messages: Array<{ body_markdown: string }> = [thread.root, ...thread.replies];
  for (const m of messages) {
    const body = m.body_markdown ?? "";
    const bodyH = Math.min(ANNOTATION_BODY_MAX_PX, Math.ceil(body.length / 80) * 20);
    h += 24 + bodyH + 12;
  }
  // composer_open not present on ThreadSnapshot — composer state lives outside the model.
  // promoted_to !== null means promoted to a GitHub comment → "Promoted" badge in place of reply footer.
  if (thread.promoted_to !== null && thread.promoted_to !== undefined) {
    h += 20;
  } else {
    h += 32; // reply footer
  }
  return h;
}

export function estimateFindingHeight(finding: FlatFinding): number {
  const title = finding.title ?? "";
  const body = finding.message_markdown ?? "";
  const textH = Math.min(
    ANNOTATION_BODY_MAX_PX,
    Math.ceil((title.length + body.length) / 80) * 20,
  );
  return 48 + textH + 24;
}

function lineNumOf(line: LineSnapshot): number | null {
  return line.new_num ?? line.old_num;
}

function visBits(v: CommentVisibility): number {
  return (
    (v.hideAll ? 1 : 0) |
    (v.hideResolved ? 2 : 0) |
    (v.hideOutdated ? 4 : 0) |
    (v.hideComments ? 8 : 0) |
    (v.hideFindings ? 16 : 0) |
    (v.hideQuestions ? 32 : 0)
  );
}

/** Line count for cache invalidation when hunks grow (lazy load, poll refresh). */
export function diffLineCount(file: FileSnapshot): number {
  let n = 0;
  for (const hunk of file.hunks) {
    n += hunk.lines.length;
  }
  return n;
}

/** Fingerprint for cross-file model cache — busts when any file diff changes. */
export function filesRenderFingerprint(files: FileSnapshot[]): string {
  return files
    .map((f) => `${f.path}:${f.cache_key}:${diffLineCount(f)}:${f.is_lazy_stub ? 1 : 0}`)
    .join("|");
}

// Per-file block cache keyed by path (NOT by FileSnapshot object identity —
// every poll deserializes fresh objects, so a WeakMap key would miss on every
// snapshot and rebuild all blocks whenever any one file changed). The inner
// modelKey carries the content hash + render inputs, so a stale entry can
// never be returned; pruned against the current file list in
// getCrossFileModel.
const _blockCache = new Map<string, Map<string, FileBlock>>();
const BLOCK_CACHE_PER_PATH = 3;

/** Drop cached blocks for files no longer present (cache only grows while
 * the file list does). */
function pruneBlockCache(files: FileSnapshot[]): void {
  if (_blockCache.size <= files.length) return;
  const live = new Set(files.map((f) => f.path));
  for (const path of _blockCache.keys()) {
    if (!live.has(path)) _blockCache.delete(path);
  }
}

export function getFileBlock(input: RenderModelInputs): FileBlock {
  const { file, fileIndex, viewMode, mode, annotationIndex, commentVisibility } = input;
  const modelKey = `${viewMode}|${annotationIndex.version}|${visBits(commentVisibility)}|${fileIndex}|${file.cache_key}|${diffLineCount(file)}|${file.is_lazy_stub ? 1 : 0}|${file.compacted ? 1 : 0}`;

  let perFile = _blockCache.get(file.path);
  if (!perFile) {
    perFile = new Map<string, FileBlock>();
    _blockCache.set(file.path, perFile);
  }
  const cached = perFile.get(modelKey);
  if (cached) return cached;

  const unifiedPairsByHunk: UnifiedPair[][] = file.hunks.map(computeUnifiedPairs);
  const splitRowsByHunk: SplitRow[][] = file.hunks.map((h) => splitRows(h.lines));

  const rows: CrossFileFlatRow[] = [];

  rows.push({
    type: "file-header",
    filePath: file.path,
    fileIndex,
    sourceIndex: file.source_index,
    height: FILE_HEADER_HEIGHT,
    identity: `fh:${file.path}`,
    additions: file.additions,
    deletions: file.deletions,
  });

  if (file.is_lazy_stub === true) {
    rows.push({
      type: "lazy-stub",
      filePath: file.path,
      fileIndex,
      sourceIndex: file.source_index,
      height: estimateLazyStubHeight(file),
      identity: `lazy:${file.path}`,
    });
  } else if (file.compacted === true) {
    rows.push({
      type: "compacted-stub",
      filePath: file.path,
      fileIndex,
      sourceIndex: file.source_index,
      height: COMPACTED_STUB_HEIGHT,
      identity: `compact:${file.path}`,
    });
  } else if (file.hunks.length === 0) {
    rows.push({
      type: "no-changes",
      filePath: file.path,
      fileIndex,
      sourceIndex: file.source_index,
      height: NO_CHANGES_HEIGHT,
      identity: `nochanges:${file.path}`,
      renamed: file.status === "renamed",
    });
  } else {
    for (let hunkIdx = 0; hunkIdx < file.hunks.length; hunkIdx++) {
      const hunk = file.hunks[hunkIdx];
      rows.push({
        type: "hunk-header",
        filePath: file.path,
        hunkIdx,
        header: hunk.header,
        height: HUNK_HEADER_HEIGHT,
        identity: `hh:${file.path}:${hunkIdx}`,
      });

      const renderedLineNums = new Set<number>();
      const side: "unified" | "split" = viewMode === "split" ? "split" : "unified";

      if (viewMode === "unified") {
        for (let lineIdx = 0; lineIdx < hunk.lines.length; lineIdx++) {
          const line = hunk.lines[lineIdx];
          if (line.kind === "fold") {
            rows.push({
              type: "content-fold",
              filePath: file.path,
              hunkIdx,
              lineIdx,
              label: line.text || "··· folded lines ···",
              height: LINE_HEIGHT,
              identity: `cf:${file.path}:${hunkIdx}:${lineIdx}`,
            });
            continue;
          }
          rows.push({
            type: "content-unified",
            filePath: file.path,
            hunkIdx,
            lineIdx,
            height: LINE_HEIGHT,
            identity: `cu:${file.path}:${hunkIdx}:${lineIdx}`,
          });
          const ln = lineNumOf(line);
          if (ln !== null) {
            renderedLineNums.add(ln);
            const skipDel =
              line.kind === "del" && hunk.lines.some((l) => l.new_num === ln);
            const findings = findingsForLine(
              annotationIndex,
              file.path,
              hunkIdx,
              ln,
              hunk.lines,
              skipDel,
              mode,
            );
            for (const f of findings) {
              rows.push({
                type: "inline-finding",
                filePath: file.path,
                hunkIdx,
                findingId: f.id,
                side,
                height: estimateFindingHeight(f),
                identity: `if:${f.id}`,
              });
            }
            const threads = threadsForLine(
              annotationIndex,
              file.path,
              hunkIdx,
              ln,
              hunk.lines,
              commentVisibility,
            ).filter((t) => {
              if (line.kind === "del" && hunk.lines.some((l) => l.new_num === t.line)) {
                return false;
              }
              return true;
            });
            for (const t of threads) {
              rows.push({
                type: "inline-thread",
                filePath: file.path,
                hunkIdx,
                threadId: t.id,
                side,
                height: estimateThreadHeight(t),
                identity: `it:${t.id}`,
              });
            }
          }
        }
      } else {
        const sRows = splitRowsByHunk[hunkIdx];
        for (let splitRowIdx = 0; splitRowIdx < sRows.length; splitRowIdx++) {
          const r = sRows[splitRowIdx];
          rows.push({
            type: "content-split",
            filePath: file.path,
            hunkIdx,
            splitRowIdx,
            height: LINE_HEIGHT,
            identity: `cs:${file.path}:${hunkIdx}:${splitRowIdx}`,
          });
          const leftLn = r.left ? lineNumOf(r.left) : null;
          const rightLn = r.right ? lineNumOf(r.right) : null;
          if (leftLn !== null) renderedLineNums.add(leftLn);
          if (rightLn !== null) renderedLineNums.add(rightLn);

          const findings = findingsForSplitRow(
            annotationIndex,
            file.path,
            hunkIdx,
            leftLn,
            rightLn,
            hunk.lines,
            mode,
          );
          for (const f of findings) {
            rows.push({
              type: "inline-finding",
              filePath: file.path,
              hunkIdx,
              findingId: f.id,
              side,
              height: estimateFindingHeight(f),
              identity: `if:${f.id}`,
            });
          }
          // Threads: dedup across left/right by id, prefer right (matches findingsForSplitRow pattern).
          const seenThreads = new Set<string>();
          const collected: ThreadSnapshot[] = [];
          for (const ln of [rightLn, leftLn]) {
            if (ln === null) continue;
            const ts = threadsForLine(
              annotationIndex,
              file.path,
              hunkIdx,
              ln,
              hunk.lines,
              commentVisibility,
            );
            for (const t of ts) {
              if (seenThreads.has(t.id)) continue;
              seenThreads.add(t.id);
              collected.push(t);
            }
          }
          for (const t of collected) {
            rows.push({
              type: "inline-thread",
              filePath: file.path,
              hunkIdx,
              threadId: t.id,
              side,
              height: estimateThreadHeight(t),
              identity: `it:${t.id}`,
            });
          }
        }
      }

      // Hunk-level findings (no line anchor)
      const hunkFindings = hunkLevelFindings(annotationIndex, file.path, hunkIdx, hunk, mode);
      const seenFindingIds = new Set<string>();
      for (const f of hunkFindings) {
        if (seenFindingIds.has(f.id)) continue;
        seenFindingIds.add(f.id);
        rows.push({
          type: "fallback-finding",
          filePath: file.path,
          hunkIdx,
          findingId: f.id,
          side,
          height: estimateFindingHeight(f),
          identity: `ff:${f.id}`,
        });
      }
      // Fallback findings (line-anchored but not rendered inline)
      const fbFindings = fallbackFindings(annotationIndex, file.path, hunkIdx, hunk, hunk.lines, mode);
      for (const f of fbFindings) {
        if (seenFindingIds.has(f.id)) continue;
        seenFindingIds.add(f.id);
        rows.push({
          type: "fallback-finding",
          filePath: file.path,
          hunkIdx,
          findingId: f.id,
          side,
          height: estimateFindingHeight(f),
          identity: `ff:${f.id}`,
        });
      }
      // Fallback threads (anchored to lines not rendered)
      const fbThreads = fallbackThreadsForHunk(
        annotationIndex,
        file.path,
        hunkIdx,
        hunk,
        renderedLineNums,
        commentVisibility,
      );
      for (const t of fbThreads) {
        rows.push({
          type: "fallback-thread",
          filePath: file.path,
          hunkIdx,
          threadId: t.id,
          side,
          height: estimateThreadHeight(t),
          identity: `ft:${t.id}`,
        });
      }
    }
  }

  const cumulativeOffsets = new Array<number>(rows.length + 1);
  cumulativeOffsets[0] = 0;
  for (let i = 0; i < rows.length; i++) {
    cumulativeOffsets[i + 1] = cumulativeOffsets[i] + rows[i].height;
  }
  const totalHeight = cumulativeOffsets[rows.length];

  const block: FileBlock = {
    filePath: file.path,
    fileIndex,
    modelKey,
    rows,
    cumulativeOffsets,
    totalHeight,
    unifiedPairsByHunk,
    splitRowsByHunk,
  };
  // Insertion-order eviction: oldest render variant goes first (typically a
  // stale annotation version or the other view mode).
  while (perFile.size >= BLOCK_CACHE_PER_PATH) {
    const oldest = perFile.keys().next().value;
    if (oldest === undefined) break;
    perFile.delete(oldest);
  }
  perFile.set(modelKey, block);
  return block;
}

// ---------------- Step B: Cross-file model ----------------

export interface CrossFileModel {
  identity: string;
  rows: CrossFileFlatRow[];
  cumulativeOffsets: number[];
  totalHeight: number;
  fileStartRow: Map<string, number>;
  /** filePath → row index of each hunk's header row, indexed by hunkIdx */
  hunkStartRow: Map<string, number[]>;
  rowFile: Uint32Array;
  threadRowIndex(threadId: string): number | null;
  findingRowIndex(findingId: string): number | null;
  unifiedPairsByFile: Map<string, UnifiedPair[][]>;
  splitRowsByFile: Map<string, SplitRow[][]>;
}

export interface CrossFileInputs {
  files: FileSnapshot[];
  viewMode: "unified" | "split";
  mode: string;
  annotationIndex: AnnotationIndex;
  commentVisibility: CommentVisibility;
  snapshotKey: string;
}

const CROSS_FILE_LRU_LIMIT = 4;
const _crossFileLru = new Map<string, CrossFileModel>();

function emptyCrossFileModel(identity: string): CrossFileModel {
  return {
    identity,
    rows: [],
    cumulativeOffsets: [0],
    totalHeight: 0,
    fileStartRow: new Map(),
    hunkStartRow: new Map(),
    rowFile: new Uint32Array(0),
    threadRowIndex: () => null,
    findingRowIndex: () => null,
    unifiedPairsByFile: new Map(),
    splitRowsByFile: new Map(),
  };
}

export function getCrossFileModel(input: CrossFileInputs): CrossFileModel {
  const { files, viewMode, mode, annotationIndex, commentVisibility, snapshotKey } = input;
  const identity = `${snapshotKey}|${viewMode}|${annotationIndex.version}|${visBits(commentVisibility)}|${filesRenderFingerprint(files)}`;

  const cached = _crossFileLru.get(identity);
  if (cached) {
    // Mark recently used by moving to end.
    _crossFileLru.delete(identity);
    _crossFileLru.set(identity, cached);
    return cached;
  }

  if (files.length === 0) {
    const model = emptyCrossFileModel(identity);
    insertLru(identity, model);
    return model;
  }

  pruneBlockCache(files);
  const blocks: FileBlock[] = new Array(files.length);
  let totalRowCount = 0;
  for (let i = 0; i < files.length; i++) {
    blocks[i] = getFileBlock({
      file: files[i],
      fileIndex: i,
      viewMode,
      mode,
      annotationIndex,
      commentVisibility,
    });
    totalRowCount += blocks[i].rows.length;
  }

  const rows: CrossFileFlatRow[] = new Array(totalRowCount);
  const rowFile = new Uint32Array(totalRowCount);
  const cumulativeOffsets = new Array<number>(totalRowCount + 1);
  cumulativeOffsets[0] = 0;
  const fileStartRow = new Map<string, number>();
  const hunkStartRow = new Map<string, number[]>();
  const threadIdx = new Map<string, number>();
  const findingIdx = new Map<string, number>();
  const unifiedPairsByFile = new Map<string, UnifiedPair[][]>();
  const splitRowsByFile = new Map<string, SplitRow[][]>();

  let writeIdx = 0;
  for (let fi = 0; fi < blocks.length; fi++) {
    const block = blocks[fi];
    fileStartRow.set(block.filePath, writeIdx);
    hunkStartRow.set(block.filePath, []);
    unifiedPairsByFile.set(block.filePath, block.unifiedPairsByHunk);
    splitRowsByFile.set(block.filePath, block.splitRowsByHunk);
    for (let j = 0; j < block.rows.length; j++) {
      const row = block.rows[j];
      rows[writeIdx] = row;
      rowFile[writeIdx] = fi;
      cumulativeOffsets[writeIdx + 1] = cumulativeOffsets[writeIdx] + row.height;
      if (row.type === "hunk-header") {
        hunkStartRow.get(block.filePath)!.push(writeIdx);
      } else if (row.type === "inline-thread" || row.type === "fallback-thread") {
        threadIdx.set(row.threadId, writeIdx);
      } else if (row.type === "inline-finding" || row.type === "fallback-finding") {
        findingIdx.set(row.findingId, writeIdx);
      }
      writeIdx++;
    }
  }
  const totalHeight = cumulativeOffsets[totalRowCount];

  const model: CrossFileModel = {
    identity,
    rows,
    cumulativeOffsets,
    totalHeight,
    fileStartRow,
    hunkStartRow,
    rowFile,
    threadRowIndex: (id) => {
      const v = threadIdx.get(id);
      return v === undefined ? null : v;
    },
    findingRowIndex: (id) => {
      const v = findingIdx.get(id);
      return v === undefined ? null : v;
    },
    unifiedPairsByFile,
    splitRowsByFile,
  };

  insertLru(identity, model);
  return model;
}

function insertLru(identity: string, model: CrossFileModel): void {
  _crossFileLru.set(identity, model);
  while (_crossFileLru.size > CROSS_FILE_LRU_LIMIT) {
    const oldest = _crossFileLru.keys().next().value;
    if (oldest === undefined) break;
    _crossFileLru.delete(oldest);
  }
}

// ── applyCollapsedFiles memo ──────────────────────────────────────────────
// Cache up to 2 results (current collapse state + last expand/collapse op).
// Key: `${model.identity}::${sorted collapsed paths joined by \0}`.
// Returns the identical object reference on a hit → downstream $derived
// consumers see no change and skip re-render.
interface CollapsedCacheEntry {
  key: string;
  result: CrossFileModel;
}
const _collapsedCache: CollapsedCacheEntry[] = [];
const COLLAPSED_CACHE_SIZE = 2;

function collapsedCacheKey(modelIdentity: string, collapsedPaths: ReadonlySet<string>): string {
  if (collapsedPaths.size === 0) return `${modelIdentity}::`;
  // Sort for deterministic key regardless of insertion order.
  return `${modelIdentity}::${[...collapsedPaths].sort().join("\x00")}`;
}

/** Hide diff body rows for collapsed files; keeps each file-header row. */
export function applyCollapsedFiles(
  model: CrossFileModel,
  collapsedPaths: ReadonlySet<string>,
): CrossFileModel {
  if (collapsedPaths.size === 0) return model;

  const memoKey = collapsedCacheKey(model.identity, collapsedPaths);
  for (const entry of _collapsedCache) {
    if (entry.key === memoKey) return entry.result;
  }

  const filteredRows: CrossFileFlatRow[] = [];
  const filteredRowFile: number[] = [];
  let skipBody = false;
  for (let i = 0; i < model.rows.length; i++) {
    const row = model.rows[i];
    if (row.type === "file-header") {
      skipBody = collapsedPaths.has(row.filePath);
      filteredRows.push(row);
      filteredRowFile.push(model.rowFile[i]);
    } else if (!skipBody) {
      filteredRows.push(row);
      filteredRowFile.push(model.rowFile[i]);
    }
  }

  if (filteredRows.length === model.rows.length) return model;

  const rowCount = filteredRows.length;
  const cumulativeOffsets = new Array<number>(rowCount + 1);
  cumulativeOffsets[0] = 0;
  const fileStartRow = new Map<string, number>();
  const hunkStartRow = new Map<string, number[]>();
  const threadIdx = new Map<string, number>();
  const findingIdx = new Map<string, number>();
  const rowFile = new Uint32Array(rowCount);

  for (let i = 0; i < rowCount; i++) {
    const row = filteredRows[i];
    rowFile[i] = filteredRowFile[i];
    cumulativeOffsets[i + 1] = cumulativeOffsets[i] + row.height;
    if (row.type === "file-header") {
      fileStartRow.set(row.filePath, i);
      hunkStartRow.set(row.filePath, []);
    } else if (row.type === "hunk-header") {
      hunkStartRow.get(row.filePath)!.push(i);
    } else if (row.type === "inline-thread" || row.type === "fallback-thread") {
      threadIdx.set(row.threadId, i);
    } else if (row.type === "inline-finding" || row.type === "fallback-finding") {
      findingIdx.set(row.findingId, i);
    }
  }

  const result: CrossFileModel = {
    // Use the full sorted-paths signature so two different sets of the same
    // size (e.g. collapse A then swap to collapse B) don't share an identity.
    identity: memoKey,
    rows: filteredRows,
    cumulativeOffsets,
    totalHeight: cumulativeOffsets[rowCount],
    fileStartRow,
    hunkStartRow,
    rowFile,
    threadRowIndex: (id) => {
      const v = threadIdx.get(id);
      return v === undefined ? null : v;
    },
    findingRowIndex: (id) => {
      const v = findingIdx.get(id);
      return v === undefined ? null : v;
    },
    unifiedPairsByFile: model.unifiedPairsByFile,
    splitRowsByFile: model.splitRowsByFile,
  };

  // Store in bounded LRU cache (newest at end, evict oldest when full).
  _collapsedCache.push({ key: memoKey, result });
  if (_collapsedCache.length > COLLAPSED_CACHE_SIZE) _collapsedCache.shift();

  return result;
}
