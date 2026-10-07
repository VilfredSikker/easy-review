import type { FileSnapshot } from "./types";

/** Minimum line-number gutter width per panel; enough for three digits.
 *  The live width is `--er-split-gutter`, set by FlatDiffView from `gutterWidthPx`. */
export const SPLIT_GUTTER_PX = 40;

/** Trailing pad inside a split annotation slot (posted card and composer). */
export const SPLIT_ANNOTATION_TRAIL_PAD_PX = 8;

/** Right pad (`pr-2`) plus clearance for the 3px add/del accent bar on the left. */
const GUTTER_PAD_PX = 8 + 6;
/** Char width to size by before the probe has measured the diff font. */
const FALLBACK_CHAR_PX = 8;

/** Largest line number any hunk shows, old or new side. */
export function maxLineNumber(files: readonly FileSnapshot[]): number {
  let max = 0;
  for (const f of files) {
    for (const h of f.hunks) {
      max = Math.max(max, h.old_start + h.old_count - 1, h.new_start + h.new_count - 1);
    }
  }
  return max;
}

/** Gutter width that fits `maxLine` without the digits running into the accent bar. */
export function gutterWidthPx(maxLine: number, charWPx: number): number {
  const digits = String(Math.max(1, maxLine)).length;
  const charW = charWPx > 0 ? charWPx : FALLBACK_CHAR_PX;
  return Math.max(SPLIT_GUTTER_PX, Math.ceil(digits * charW + GUTTER_PAD_PX));
}
