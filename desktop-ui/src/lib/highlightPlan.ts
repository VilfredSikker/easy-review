import type { HunkHighlight } from "./highlightCache";
import type { FileSnapshot, LineSnapshot, SpanSnapshot } from "./types";

interface LineRef {
  hunkIdx: number;
  lineIdx: number;
}

interface HighlightSide {
  texts: string[];
  refs: LineRef[];
}

/** Build new-file and old-file stitch buffers (skip opposite-side lines). */
export function buildHighlightSides(file: FileSnapshot): {
  newSide: HighlightSide;
  oldSide: HighlightSide;
} {
  const newSide: HighlightSide = { texts: [], refs: [] };
  const oldSide: HighlightSide = { texts: [], refs: [] };

  for (let hunkIdx = 0; hunkIdx < file.hunks.length; hunkIdx++) {
    const hunk = file.hunks[hunkIdx];
    for (let lineIdx = 0; lineIdx < hunk.lines.length; lineIdx++) {
      const line = hunk.lines[lineIdx];
      if (line.kind === "fold") continue;
      const ref = { hunkIdx, lineIdx };
      if (line.kind !== "del") {
        newSide.texts.push(line.text);
        newSide.refs.push(ref);
      }
      if (line.kind !== "add") {
        oldSide.texts.push(line.text);
        oldSide.refs.push(ref);
      }
    }
  }

  return { newSide, oldSide };
}

/** Map per-side highlight output back onto diff hunk line indices. */
export function spansToHunksFromSides(
  file: FileSnapshot,
  newSide: HighlightSide,
  newSpans: SpanSnapshot[][],
  oldSide: HighlightSide,
  oldSpans: SpanSnapshot[][],
): HunkHighlight[] {
  const byLine = new Map<string, SpanSnapshot[]>();

  for (let i = 0; i < newSide.refs.length; i++) {
    const { hunkIdx, lineIdx } = newSide.refs[i];
    const line = file.hunks[hunkIdx].lines[lineIdx];
    if (line.kind !== "del") {
      byLine.set(`${hunkIdx}:${lineIdx}`, newSpans[i] ?? []);
    }
  }

  for (let i = 0; i < oldSide.refs.length; i++) {
    const { hunkIdx, lineIdx } = oldSide.refs[i];
    const line = file.hunks[hunkIdx].lines[lineIdx];
    if (line.kind === "del") {
      byLine.set(`${hunkIdx}:${lineIdx}`, oldSpans[i] ?? []);
    }
  }

  return file.hunks.map((hunk, hunk_index) => ({
    hunk_index,
    lines: hunk.lines.map((line, lineIdx) => {
      if (line.kind === "fold") return [] as SpanSnapshot[];
      return byLine.get(`${hunk_index}:${lineIdx}`) ?? [];
    }),
  }));
}

/** True when span snapshots include at least one token color. */
export function hasColoredSyntaxSpans(spans: SpanSnapshot[] | undefined): boolean {
  return spans?.some((s) => s.color) ?? false;
}

/** True when a line has no usable syntax colors (for re-apply after poll). */
export function lineNeedsSyntaxSpans(line: LineSnapshot): boolean {
  if (line.kind === "fold") return false;
  return !hasColoredSyntaxSpans(line.spans);
}

export function fileNeedsSyntaxSpans(file: FileSnapshot): boolean {
  for (const hunk of file.hunks) {
    for (const line of hunk.lines) {
      if (lineNeedsSyntaxSpans(line)) return true;
    }
  }
  return false;
}

/** True when at least one non-fold line has a colored syntax span. */
export function fileHasColoredSpans(file: FileSnapshot): boolean {
  for (const hunk of file.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "fold") continue;
      if (hasColoredSyntaxSpans(line.spans)) return true;
    }
  }
  return false;
}

export function fileHasDeletions(file: FileSnapshot): boolean {
  for (const hunk of file.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "del") return true;
    }
  }
  return false;
}

/** Shallow equality for syntax span arrays (text + color per token). */
export function syntaxSpansEqual(
  a: SpanSnapshot[] | undefined,
  b: SpanSnapshot[] | undefined,
): boolean {
  if (a === b) return true;
  if (!a || !b || a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i].text !== b[i].text || a[i].color !== b[i].color) return false;
  }
  return true;
}

/**
 * True when cached hunks would add colored spans to at least one line that lacks them.
 */
export function cacheWouldImproveFile(file: FileSnapshot, hunks: HunkHighlight[]): boolean {
  for (const hh of hunks) {
    const hunk = file.hunks[hh.hunk_index];
    if (!hunk) continue;
    for (let lineIdx = 0; lineIdx < hunk.lines.length; lineIdx++) {
      const line = hunk.lines[lineIdx];
      const nextSpans = hh.lines[lineIdx];
      if (!hasColoredSyntaxSpans(nextSpans)) continue;
      if (!hasColoredSyntaxSpans(line.spans) || !syntaxSpansEqual(line.spans, nextSpans)) {
        return true;
      }
    }
  }
  return false;
}

/**
 * Apply highlight hunks to a live file only when at least one line gains new colored spans.
 * Returns false without mutating `file.hunks` when nothing would change.
 */
export function applyHunkSpansIfChanged(file: FileSnapshot, hunks: HunkHighlight[]): boolean {
  const spansByHunkIdx = new Map<number, SpanSnapshot[][]>();
  for (const hh of hunks) spansByHunkIdx.set(hh.hunk_index, hh.lines);

  let any = false;
  const newHunks = file.hunks.map((hunk, hIdx) => {
    const spans = spansByHunkIdx.get(hIdx);
    if (!spans) return hunk;

    let hunkChanged = false;
    const newLines = hunk.lines.map((line, lIdx) => {
      const nextSpans = spans[lIdx];
      if (!hasColoredSyntaxSpans(nextSpans)) return line;
      if (syntaxSpansEqual(line.spans, nextSpans)) return line;
      hunkChanged = true;
      return { ...line, spans: nextSpans };
    });

    if (!hunkChanged) return hunk;
    any = true;
    return { ...hunk, lines: newLines };
  });

  if (!any) return false;
  file.hunks = newHunks;
  return true;
}

/**
 * Skip re-applying cached highlights only when spans are already on the live file.
 * After a backend poll replaces the snapshot, lines lose client-side spans but the
 * applied-key may still be set — do not skip in that case.
 */
export function shouldSkipHighlightApply(
  file: FileSnapshot,
  spanKeyAlreadyApplied: boolean,
): boolean {
  return spanKeyAlreadyApplied && !fileNeedsSyntaxSpans(file);
}
