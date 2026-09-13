/**
 * Where the comment composer attaches in the flat diff row list.
 *
 * The composer renders **in flow, directly below the last selected line's row**
 * — never floating over the diff — so the lines being commented on stay visible
 * and the code that follows them is pushed down instead of covered.
 *
 * Two pieces make that work, both pure so they can be tested without a DOM:
 *  - {@link findComposerAnchorRow}: which row the composer attaches to.
 *  - {@link foldRowExtras}: how the space it occupies enters the row geometry.
 */

/** Minimum shape the anchor scan needs from a flat row. */
export interface AnchorScanRow {
  filePath: string;
}

export interface ComposerAnchorQuery {
  /** Flat model rows, in render order. */
  rows: readonly AnchorScanRow[];
  /** First row belonging to the anchor file. */
  startRow: number;
  filePath: string;
  /** Last line of the selection, on the selection's own side. */
  lastLine: number;
  /**
   * Selected-side line number carried by the row at `idx`, or `null` when the
   * row is not a content row on that side (headers, annotations, the other
   * side of a modify pair).
   */
  lineAt: (idx: number) => number | null;
}

/**
 * Index of the row carrying the selection's last line — the row the composer
 * sits under. `null` when the file is not in the model or the line is not
 * rendered (compacted or lazy stub), leaving the caller to fall back to the
 * floating card.
 */
export function findComposerAnchorRow(query: ComposerAnchorQuery): number | null {
  const { rows, startRow, filePath, lastLine, lineAt } = query;
  if (startRow < 0 || startRow >= rows.length) return null;
  for (let i = startRow; i < rows.length; i++) {
    if (rows[i].filePath !== filePath) break;
    if (lineAt(i) === lastLine) return i;
  }
  return null;
}

/**
 * Fold per-row extra heights (Guide pillar padding, composer band) into the
 * base offsets.
 *
 * The extra is added to the row it belongs to, so that row's top is unchanged
 * and every row after it shifts down — the property that keeps selected lines
 * where the reader left them while the composer takes space beneath.
 */
export function foldRowExtras(
  baseOffsets: readonly number[],
  extraAt: (rowIdx: number) => number,
): number[] {
  // The row count comes from the offsets themselves, so every `baseOffsets[i+1]`
  // read below is in range — a mismatch cannot silently collapse rows to zero.
  const rowCount = Math.max(0, baseOffsets.length - 1);
  const offsets = new Array<number>(rowCount + 1);
  offsets[0] = 0;
  for (let i = 0; i < rowCount; i++) {
    const baseH = baseOffsets[i + 1] - baseOffsets[i];
    offsets[i + 1] = offsets[i] + baseH + extraAt(i);
  }
  return offsets;
}
