/**
 * Lexical "highlight all references" for the diff view (issue #69).
 *
 * Clicking an identifier token (function name, variable, constant) highlights
 * every other occurrence of that identifier across the rendered diff. This is
 * a purely lexical match — word-boundary aware, no language server.
 *
 * Word characters follow the common identifier alphabet `[A-Za-z0-9_$]`, so
 * clicking `foo` never highlights `foobar`, and `$state` / `snake_case` are
 * matched whole.
 */

import type { RenderSegment } from "./mergeWordDiffWithSyntax";

/** A render segment optionally marked as a reference-highlight match. */
export interface RefSegment extends RenderSegment {
  ref?: boolean;
}

const WORD_CHAR = /[A-Za-z0-9_$]/;

function isWordChar(ch: string | undefined): boolean {
  return ch !== undefined && WORD_CHAR.test(ch);
}

/**
 * Extract the identifier under a caret offset in `text`.
 *
 * Caret semantics: prefer the character at `offset`; if it is not a word
 * character (caret sits just past a token), fall back to the character
 * before. Pure-numeric tokens are rejected — highlighting every `2` in a
 * diff is noise, and numerals are not identifiers in any mainstream language.
 */
export function identifierAt(text: string, offset: number): string | null {
  if (text.length === 0 || offset < 0 || offset > text.length) return null;
  let i = offset;
  if (i >= text.length || !isWordChar(text[i])) {
    if (i > 0 && isWordChar(text[i - 1])) {
      i -= 1;
    } else {
      return null;
    }
  }
  let start = i;
  while (start > 0 && isWordChar(text[start - 1])) start--;
  let end = i + 1;
  while (end < text.length && isWordChar(text[end])) end++;
  const word = text.slice(start, end);
  if (!/[A-Za-z_$]/.test(word)) return null;
  return word;
}

/**
 * Find all word-boundary occurrences of `identifier` in `text`.
 * Returns sorted, non-overlapping `[start, end)` ranges.
 */
export function findIdentifierRanges(
  text: string,
  identifier: string,
): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  if (identifier.length === 0) return ranges;
  let from = 0;
  while (from <= text.length - identifier.length) {
    const i = text.indexOf(identifier, from);
    if (i === -1) break;
    const end = i + identifier.length;
    const boundaryBefore = i === 0 || !isWordChar(text[i - 1]);
    const boundaryAfter = end >= text.length || !isWordChar(text[end]);
    if (boundaryBefore && boundaryAfter) {
      ranges.push([i, end]);
      from = end;
    } else {
      from = i + 1;
    }
  }
  return ranges;
}

/**
 * Split render segments at the boundaries of `identifier` matches over the
 * concatenated line text, marking matched slices with `ref: true`. Existing
 * word-diff / syntax-color attributes are preserved on each slice, so the
 * reference highlight composes with intra-line change backgrounds and token
 * colors. Returns the input array unchanged when there is no match (cheap
 * common case for non-matching lines).
 */
export function splitSegmentsByIdentifier(
  segments: RenderSegment[],
  identifier: string,
): RefSegment[] {
  const full = segments.map((s) => s.text).join("");
  const ranges = findIdentifierRanges(full, identifier);
  if (ranges.length === 0) return segments;

  const out: RefSegment[] = [];
  let pos = 0;
  let ri = 0;
  for (const seg of segments) {
    const segEnd = pos + seg.text.length;
    let cursor = pos;
    while (cursor < segEnd) {
      while (ri < ranges.length && ranges[ri][1] <= cursor) ri++;
      const range = ri < ranges.length ? ranges[ri] : null;
      if (!range || range[0] >= segEnd) {
        out.push({ ...seg, text: full.slice(cursor, segEnd) });
        cursor = segEnd;
        break;
      }
      const [rs, re] = range;
      if (rs > cursor) {
        out.push({ ...seg, text: full.slice(cursor, rs) });
        cursor = rs;
      }
      const sliceEnd = Math.min(re, segEnd);
      out.push({ ...seg, text: full.slice(cursor, sliceEnd), ref: true });
      cursor = sliceEnd;
    }
    pos = segEnd;
  }
  return out.filter((s) => s.text.length > 0);
}

/**
 * Character offset of a mouse event's caret position within `container`,
 * counted over the container's concatenated text content. Returns null when
 * the point does not resolve to a position inside the container.
 *
 * Uses `caretRangeFromPoint` (WebKit/Chromium — what Tauri webviews ship)
 * with a `caretPositionFromPoint` fallback (Firefox / the CSSOM standard).
 */
export function caretTextOffset(e: MouseEvent, container: HTMLElement): number | null {
  const doc = document as Document & {
    caretRangeFromPoint?: (x: number, y: number) => Range | null;
    caretPositionFromPoint?: (
      x: number,
      y: number,
    ) => { offsetNode: Node; offset: number } | null;
  };

  let node: Node | null = null;
  let offset = 0;
  if (typeof doc.caretRangeFromPoint === "function") {
    const r = doc.caretRangeFromPoint(e.clientX, e.clientY);
    if (!r) return null;
    node = r.startContainer;
    offset = r.startOffset;
  } else if (typeof doc.caretPositionFromPoint === "function") {
    const p = doc.caretPositionFromPoint(e.clientX, e.clientY);
    if (!p) return null;
    node = p.offsetNode;
    offset = p.offset;
  } else {
    return null;
  }

  if (!node || !container.contains(node)) return null;
  const range = document.createRange();
  range.setStart(container, 0);
  try {
    range.setEnd(node, offset);
  } catch {
    return null;
  }
  return range.toString().length;
}
