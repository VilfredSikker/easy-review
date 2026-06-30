/**
 * Usage collection + presentation helpers for the reference-highlight
 * feature (issue #69): the scrollbar overview ruler and the Cmd+click
 * usages popover.
 *
 * Everything here is pure. The component layer (FlatDiffView and the
 * popover/ruler components) feeds in row/line data plus geometry and renders
 * the result — keeping the matching, grouping, trimming, and ruler math
 * testable without a DOM.
 */

import {
  findMatchRanges,
  IDENTIFIER_MATCH_OPTIONS,
  type MatchOptions,
} from "./referenceHighlight";

/** A diff line that may contain identifier matches, in flat-row coordinates. */
export interface UsageSource {
  /** Index into the cross-file flat row array (drives scroll position). */
  rowIdx: number;
  filePath: string;
  /** Display line number (new side preferred, old side for deletions). */
  lineNum: number | null;
  text: string;
  /**
   * Hunk index within the file, when known. `usageContext` refuses to cross
   * hunk boundaries — adjacent hunks are adjacent in render order but not in
   * the underlying file, so their lines are not real context for each other.
   */
  hunkIdx?: number;
  /**
   * Line index within its hunk, when known. Together with `filePath`/`hunkIdx`
   * this is a stable anchor that survives collapse: a usage in a collapsed
   * file has no rendered row (`rowIdx === -1`), so jumping re-resolves the row
   * from this anchor after the file is expanded.
   */
  lineIdx?: number;
}

/** A line with at least one word-boundary match of the active identifier. */
export interface UsageLine extends UsageSource {
  /** Sorted, non-overlapping `[start, end)` match ranges within `text`. */
  ranges: Array<[number, number]>;
}

/** Result of `collectMatches`: matched lines plus range-level totals. */
export interface MatchResult {
  lines: UsageLine[];
  /** Individual match ranges collected (≤ `maxMatches`). */
  total: number;
  /** True when collection stopped at `maxMatches` before exhausting sources. */
  capped: boolean;
}

/**
 * Filter `sources` down to lines matching `query` under `opts`, counting
 * individual ranges. Input order is preserved (callers supply rows in render
 * order, so results stay grouped by file). Collection stops once `maxMatches`
 * ranges have been gathered — a one-letter Cmd+F query over a huge diff must
 * not build an unbounded match list.
 */
export function collectMatches(
  sources: Iterable<UsageSource>,
  query: string,
  opts: MatchOptions,
  maxMatches = 5000,
): MatchResult {
  const lines: UsageLine[] = [];
  let total = 0;
  let capped = false;
  if (query.length === 0) return { lines, total, capped };
  for (const s of sources) {
    if (total >= maxMatches) {
      capped = true;
      break;
    }
    const ranges = findMatchRanges(s.text, query, opts);
    if (ranges.length === 0) continue;
    let kept = ranges;
    if (total + ranges.length > maxMatches) {
      kept = ranges.slice(0, maxMatches - total);
      capped = true;
    }
    lines.push({ ...s, ranges: kept });
    total += kept.length;
    if (capped) break;
  }
  return { lines, total, capped };
}

/**
 * Filter `sources` down to lines containing whole-word occurrences of
 * `identifier`. Input order is preserved (callers supply rows in render
 * order, so results stay grouped by file).
 */
export function collectUsageLines(
  sources: Iterable<UsageSource>,
  identifier: string,
): UsageLine[] {
  return collectMatches(sources, identifier, IDENTIFIER_MATCH_OPTIONS, Infinity).lines;
}

export interface UsageGroup {
  filePath: string;
  usages: UsageLine[];
}

export interface GroupedUsages {
  groups: UsageGroup[];
  /** Total usages before the cap. */
  total: number;
  /** Usages actually included across `groups` (= min(total, cap)). */
  shown: number;
}

/**
 * Group consecutive usages by file, keeping at most `cap` usages overall
 * (the cap applies across groups, in input order). Files whose usages fall
 * entirely past the cap are omitted.
 */
export function groupUsagesByFile(usages: UsageLine[], cap = 100): GroupedUsages {
  const groups: UsageGroup[] = [];
  let shown = 0;
  for (const u of usages) {
    if (shown >= cap) break;
    const last = groups[groups.length - 1];
    if (last && last.filePath === u.filePath) {
      last.usages.push(u);
    } else {
      groups.push({ filePath: u.filePath, usages: [u] });
    }
    shown++;
  }
  return { groups, total: usages.length, shown };
}

/** A one-line code preview split around the emphasized match. */
export interface UsagePreview {
  prefix: string;
  match: string;
  suffix: string;
}

/**
 * Build a trimmed single-line preview around the first match range.
 * Leading indentation is dropped; a long prefix is left-truncated with an
 * ellipsis so the match stays visible without scrolling; the suffix is cut
 * to keep the whole preview within `maxTotal` characters. The total budget
 * intentionally exceeds the popover's visible width — overflow scrolls
 * horizontally instead of truncating, so the budget only guards against
 * pathological lines (minified bundles), not normal code.
 */
export function usagePreview(
  text: string,
  range: [number, number],
  maxPrefix = 24,
  maxTotal = 240,
): UsagePreview {
  const [start, end] = range;
  // Drop leading whitespace (indentation carries no information in a list).
  let lead = 0;
  while (lead < start && (text[lead] === " " || text[lead] === "\t")) lead++;
  let prefix = text.slice(lead, start);
  const match = text.slice(start, end);
  let suffix = text.slice(end);
  if (prefix.length > maxPrefix) {
    prefix = "…" + prefix.slice(prefix.length - (maxPrefix - 1));
  }
  const budget = Math.max(0, maxTotal - prefix.length - match.length);
  if (suffix.length > budget) {
    suffix = suffix.slice(0, Math.max(0, budget - 1)) + "…";
  }
  return { prefix, match, suffix };
}

/** One line of surrounding context for a reference usage. */
export interface UsageContextLine {
  rowIdx: number;
  lineNum: number | null;
  text: string;
  /** True for the usage line itself (the one carrying the match). */
  isMatch: boolean;
}

/**
 * Surrounding context for `usage`: up to `contextLines` lines above and below
 * it, taken from `sources` (the flat render-order line list the usage was
 * collected from). Context never crosses file boundaries, and never crosses
 * hunk boundaries when sources carry `hunkIdx` — adjacent hunks are adjacent
 * on screen but not in the file. At the first/last line of a hunk or of the
 * whole list, fewer lines are returned.
 *
 * The usage is located by its `(filePath, hunkIdx, lineIdx)` anchor when those
 * are known, falling back to `filePath` + `rowIdx` + `text` + `lineNum`. The
 * anchor matters for collapsed files: every line there shares `rowIdx === -1`,
 * so two lines with the same text and line number in different hunks would
 * otherwise be indistinguishable. (Split rows can also share a `rowIdx` between
 * their left and right sides, disambiguated by `text`/`lineNum`/`lineIdx`.)
 * When the usage cannot be found (e.g. the diff refreshed underneath a stale
 * reference), the usage line itself is returned alone so callers always have
 * something to render.
 */
export function usageContext(
  sources: UsageSource[],
  usage: Pick<UsageSource, "rowIdx" | "filePath" | "lineNum" | "text" | "hunkIdx" | "lineIdx">,
  contextLines = 2,
): UsageContextLine[] {
  const idx = sources.findIndex(
    (s) =>
      s.filePath === usage.filePath &&
      s.rowIdx === usage.rowIdx &&
      s.hunkIdx === usage.hunkIdx &&
      s.lineIdx === usage.lineIdx &&
      s.text === usage.text &&
      s.lineNum === usage.lineNum,
  );
  if (idx === -1) {
    return [{ rowIdx: usage.rowIdx, lineNum: usage.lineNum, text: usage.text, isMatch: true }];
  }
  const anchor = sources[idx];
  const sameRun = (s: UsageSource): boolean =>
    s.filePath === anchor.filePath && s.hunkIdx === anchor.hunkIdx;
  let start = idx;
  while (start > 0 && idx - start < contextLines && sameRun(sources[start - 1])) start--;
  let end = idx;
  while (end < sources.length - 1 && end - idx < contextLines && sameRun(sources[end + 1])) end++;
  const out: UsageContextLine[] = [];
  for (let i = start; i <= end; i++) {
    const s = sources[i];
    out.push({ rowIdx: s.rowIdx, lineNum: s.lineNum, text: s.text, isMatch: i === idx });
  }
  return out;
}

/** One mark on the overview ruler, in ruler-local pixels. */
export interface RulerMark {
  /** Flat row index of the (first) match this mark represents. */
  rowIdx: number;
  topPx: number;
  /** Matched rows merged into this mark (≥ 1; > 1 for a dense cluster). */
  count: number;
}

/**
 * Map matched rows to overview-ruler marks.
 *
 * Positioning: a row at content offset `offsetPx` (top of the row in the
 * scrollable content, whose full height is `totalContentPx`) maps to
 * `offsetPx / totalContentPx * rulerPx`, clamped so the mark stays inside the
 * ruler. Marks that would overlap (closer than `markHeightPx`) are merged
 * into the earlier mark (its `count` accumulates the cluster size), so a
 * dense cluster renders as one solid block instead of thousands of DOM nodes.
 *
 * `rows` must be sorted by `offsetPx` ascending (callers iterate flat rows in
 * order, which guarantees this).
 */
export function buildRulerMarks(
  rows: Array<{ rowIdx: number; offsetPx: number }>,
  totalContentPx: number,
  rulerPx: number,
  markHeightPx = 3,
): RulerMark[] {
  const marks: RulerMark[] = [];
  if (totalContentPx <= 0 || rulerPx <= 0) return marks;
  const maxTop = Math.max(0, rulerPx - markHeightPx);
  let lastTop = -Infinity;
  for (const row of rows) {
    const top = Math.min(
      maxTop,
      Math.max(0, Math.round((row.offsetPx / totalContentPx) * rulerPx)),
    );
    if (top - lastTop < markHeightPx) {
      const last = marks[marks.length - 1];
      if (last) last.count++;
      continue;
    }
    marks.push({ rowIdx: row.rowIdx, topPx: top, count: 1 });
    lastTop = top;
  }
  return marks;
}

/**
 * Clamp a popover's top-left corner so a `w`×`h` box stays inside the
 * `vw`×`vh` viewport with `pad` breathing room. When the box can't fit, the
 * top/left edge wins (content scrolls; the anchor edge stays reachable).
 */
export function clampPopoverPosition(
  x: number,
  y: number,
  w: number,
  h: number,
  vw: number,
  vh: number,
  pad = 8,
): { left: number; top: number } {
  const left = Math.max(pad, Math.min(x, vw - w - pad));
  const top = Math.max(pad, Math.min(y, vh - h - pad));
  return { left, top };
}
