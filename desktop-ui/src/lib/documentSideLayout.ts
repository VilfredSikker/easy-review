import type { CrossFileFlatRow } from "./diffRenderModel";
import type { DocumentBlock } from "./documentPreview";
import type { FileSnapshot, LineSnapshot } from "./types";

/** A rendered block and the flat rows of raw lines it sits beside. */
export interface SideBlockSpan {
  key: string;
  filePath: string;
  block: DocumentBlock;
  /** Global row index of the block's first raw line. */
  firstRow: number;
  /** Last row the block covers: everything up to the next block, a hunk
   *  break or the file end, so padding lands after trailing blank lines,
   *  deleted lines and comment cards rather than in the middle of them. */
  lastRow: number;
  /** True when any of its lines is added in this diff. */
  changed: boolean;
}

function newSideLine(row: CrossFileFlatRow, file: FileSnapshot): LineSnapshot | null {
  if (row.type !== "content-unified") return null;
  const line = file.hunks[row.hunkIdx]?.lines[row.lineIdx];
  if (!line || line.kind === "del" || line.kind === "fold" || line.new_num === null) return null;
  return line;
}

function endsSpan(row: CrossFileFlatRow): boolean {
  return row.type === "hunk-header" || row.type === "content-fold" ||
    row.type === "fallback-thread" || row.type === "fallback-finding";
}

/**
 * Match each block to the rows showing its new-side lines. Blocks with no line
 * in the diff are skipped: an edited document shows only its changed sections.
 */
export function sideBlockSpans(
  blocks: readonly DocumentBlock[],
  rows: readonly CrossFileFlatRow[],
  startRow: number,
  file: FileSnapshot,
): SideBlockSpan[] {
  const spans: SideBlockSpan[] = [];
  let blockIdx = 0;
  let current = null as SideBlockSpan | null;
  let i = startRow;
  for (; i < rows.length && rows[i].filePath === file.path; i++) {
    const raw = newSideLine(rows[i], file);
    if (raw === null) continue;
    const line = raw.new_num ?? 0;
    // New-side line numbers only grow down the file, so one forward walk does.
    while (blockIdx < blocks.length && blocks[blockIdx].endLine < line) blockIdx++;
    const block = blocks[blockIdx];
    if (!block || block.startLine > line) continue;
    if (current?.block === block) {
      current.lastRow = i;
      current.changed ||= raw.kind === "add";
      continue;
    }
    current = {
      key: `${file.path}:${block.startLine}-${block.endLine}`,
      filePath: file.path,
      block,
      firstRow: i,
      lastRow: i,
      changed: raw.kind === "add",
    };
    spans.push(current);
  }
  spans.forEach((span, n) => {
    const limit = spans[n + 1]?.firstRow ?? i;
    while (span.lastRow + 1 < limit && !endsSpan(rows[span.lastRow + 1])) span.lastRow++;
  });
  return spans;
}

/**
 * Extra height after each block's last row, so the raw lines are never shorter
 * than the rendered block beside them. Keyed by row index; read from offsets
 * that exclude this padding, so it cannot feed back into itself.
 */
export function sideBlockPadding(
  spans: readonly SideBlockSpan[],
  offsets: readonly number[],
  renderedHeight: (key: string) => number | undefined,
): Map<number, number> {
  const pad = new Map<number, number>();
  for (const span of spans) {
    const height = renderedHeight(span.key);
    if (height === undefined) continue;
    const rawHeight = offsets[span.lastRow + 1] - offsets[span.firstRow];
    if (height > rawHeight) pad.set(span.lastRow, height - rawHeight);
  }
  return pad;
}
