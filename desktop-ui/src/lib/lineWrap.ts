/**
 * Word-wrap math for diff code cells (issue: 50/50 split view).
 *
 * The diff renders in a monospace font, cells wrap with
 * `white-space: pre-wrap; word-break: break-all`, and continuation lines get a
 * hanging indent via `padding-left: calc(0.75rem + Nch); text-indent: -Nch`.
 * Under break-all the browser fills each line box to capacity, so the visual
 * line count of a wrapped line is deterministic from its column count — that
 * lets the render model predict row heights without measuring DOM. Rendered
 * rows are still observed by the measured-height overlay in FlatDiffView, so
 * any residual drift (tab-stop quirks, trailing-space hang) self-corrects.
 *
 * Keep `hangingIndentStyle` and `wrappedLineCount` in lockstep: the CSS the
 * rows apply must match the arithmetic the model predicts with.
 */

/** Columns taken by the diff marker prefix rendered before the code text ("+ " / "- " / "  "). */
export const CODE_PREFIX_COLS = 2;

/** Narrowest code column we ever wrap at — guards against degenerate layouts. */
export const MIN_WRAP_COLS = 24;

/** A wrapped continuation line always keeps at least this many columns, no
 *  matter how deep the hanging indent would be. */
export const MIN_CONTINUATION_COLS = 16;

/** CSS `tab-size` default — app.css doesn't override it. */
const TAB_WIDTH = 8;

/** Leading whitespace characters (spaces/tabs), mirroring the hanging-indent rule. */
export function leadingWhitespaceChars(text: string): number {
  let n = 0;
  while (n < text.length && (text[n] === " " || text[n] === "\t")) n++;
  return n;
}

/**
 * Rendered columns of a code line including the marker prefix. Tabs advance to
 * the next tab stop measured from the start of the line box (which begins with
 * the 2-column prefix).
 */
export function lineTotalCols(text: string): number {
  let col = CODE_PREFIX_COLS;
  for (let i = 0; i < text.length; i++) {
    col += text.charCodeAt(i) === 9 ? TAB_WIDTH - (col % TAB_WIDTH) : 1;
  }
  return col;
}

/**
 * Hanging indent (in ch) applied to wrapped continuation lines, clamped so a
 * continuation line keeps at least {@link MIN_CONTINUATION_COLS} columns.
 * `wrapCols` is the cell's total column capacity; pass null when wrapping is
 * off (no clamp needed — there are no continuation lines).
 */
export function hangIndentCols(text: string, wrapCols: number | null): number {
  const raw = leadingWhitespaceChars(text) + CODE_PREFIX_COLS;
  if (wrapCols === null) return raw;
  return Math.max(0, Math.min(raw, wrapCols - MIN_CONTINUATION_COLS));
}

/**
 * Inline style for a code cell's hanging indent. All lines are padded to the
 * indent; the negative text-indent pulls the first line back so it starts at
 * the base 0.75rem padding. Matches the pre-existing leadingWS style, with the
 * indent clamped when wrapping so continuation lines can always make progress.
 */
export function hangingIndentStyle(text: string, wrapCols: number | null): string {
  const cols = hangIndentCols(text, wrapCols);
  return `padding-left: calc(0.75rem + ${cols}ch); text-indent: -${cols}ch;`;
}

/** Visual line count of one code line wrapped at `wrapCols` columns. */
export function wrappedLineCount(text: string, wrapCols: number): number {
  const total = lineTotalCols(text);
  if (total <= wrapCols) return 1;
  const continuationCols = wrapCols - hangIndentCols(text, wrapCols);
  return 1 + Math.ceil((total - wrapCols) / continuationCols);
}
