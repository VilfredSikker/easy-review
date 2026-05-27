export interface VirtualWindow {
  start: number; // first visible index (inclusive)
  end: number; // last visible index (exclusive)
  paddingTop: number; // px to apply as top spacer
  paddingBottom: number; // px to apply as bottom spacer
}

const DEFAULT_OVERSCAN = 5;

/**
 * Fixed-height mode: all rows share the same height.
 * Call this from FileTree (Phase 1b).
 */
export function windowFromScroll(
  totalItems: number,
  itemHeight: number,
  scrollTop: number,
  viewportHeight: number,
  overscan = DEFAULT_OVERSCAN,
): VirtualWindow {
  if (totalItems === 0) {
    return { start: 0, end: 0, paddingTop: 0, paddingBottom: 0 };
  }

  // Hunk may be below the visible area (scrollTop not yet reached) — clamp so
  // negative scrollTop doesn't produce negative firstVisible/lastVisible indices.
  const clampedScroll = Math.max(0, scrollTop);

  const firstVisible = Math.floor(clampedScroll / itemHeight);
  const lastVisible = Math.ceil((clampedScroll + viewportHeight) / itemHeight);

  const start = Math.max(0, firstVisible - overscan);
  const end = Math.max(start, Math.min(totalItems, lastVisible + overscan));

  const paddingTop = start * itemHeight;
  const paddingBottom = (totalItems - end) * itemHeight;

  return { start, end, paddingTop, paddingBottom };
}

/**
 * Variable-height mode: uses precomputed cumulative offsets array.
 *
 * **Contract (must match `diffRenderModel.ts` prefix-sum convention):**
 * - `cumulativeOffsets.length === rowCount + 1` (terminal entry holds `totalHeight`).
 * - `cumulativeOffsets[i]` = top of row `i` (so `cumulativeOffsets[0] === 0`).
 * - `cumulativeOffsets[rowCount] === totalHeight`.
 * - Empty input: `cumulativeOffsets = [0]` (length 1, rowCount 0).
 *
 * Returned `start` / `end` index `rowCount` rows. `end` is exclusive and may
 * equal `rowCount` but never exceed it.
 */
export function windowFromScrollVariable(
  cumulativeOffsets: number[],
  totalHeight: number,
  scrollTop: number,
  viewportHeight: number,
  overscan = DEFAULT_OVERSCAN,
): VirtualWindow {
  const rowCount = Math.max(0, cumulativeOffsets.length - 1);
  if (rowCount === 0) {
    return { start: 0, end: 0, paddingTop: 0, paddingBottom: 0 };
  }

  const clampedScroll = Math.max(0, scrollTop);

  // First row whose top edge is >= scrollTop. Step back one so the row that
  // straddles `scrollTop` is included (binary-search returns the row whose
  // top is at or after scrollTop; the row visually at scrollTop is one before
  // unless scrollTop falls exactly on a row boundary).
  const firstAtOrAfter = binarySearchLeft(cumulativeOffsets, clampedScroll);
  const firstVisible =
    firstAtOrAfter < cumulativeOffsets.length && cumulativeOffsets[firstAtOrAfter] === clampedScroll
      ? firstAtOrAfter
      : Math.max(0, firstAtOrAfter - 1);

  // First row whose top edge is >= visibleBottom. This row is the first NOT
  // visible — use it directly as exclusive `end`.
  const visibleBottom = clampedScroll + viewportHeight;
  const lastVisibleExclusive = binarySearchLeft(cumulativeOffsets, visibleBottom);

  const start = Math.max(0, firstVisible - overscan);
  const end = Math.max(start, Math.min(rowCount, lastVisibleExclusive + overscan));

  const paddingTop = cumulativeOffsets[start];
  const paddingBottom = totalHeight - cumulativeOffsets[end];

  return { start, end, paddingTop, paddingBottom };
}

export interface EffectiveGeometry {
  /** length = rowCount + 1; cumulativeOffsets[rowCount] === totalHeight */
  cumulativeOffsets: number[];
  totalHeight: number;
  rowCount: number;
}

/**
 * Returns the row index whose `[top, bottom)` range contains `offsetPx`.
 *
 * Boundary semantics:
 * - `offsetPx <= 0` → row 0 (or -1 if empty).
 * - `offsetPx >= totalHeight` → last row (`rowCount - 1`).
 * - Exact row-top boundary returns that row's index (not the predecessor).
 * - Empty geometry (`rowCount === 0`) → -1.
 */
/**
 * Map a viewport pointer Y to a document row offset (px into `effectiveGeometry`).
 *
 * `contentTopPx` is the non-scrollable band at the top of the scroll container
 * (e.g. FlatDiffView's sticky file header, `STICKY_HEADER_PX = 40`).
 */
export function rowOffsetFromViewportY(
  clientY: number,
  containerTop: number,
  scrollTopPx: number,
  contentTopPx: number,
): number {
  return clientY - containerTop - contentTopPx + scrollTopPx;
}

/**
 * Map a viewport pointer Y to a document row offset using the rendered content
 * surface's top edge as the origin.
 *
 * For FlatDiffView this should be the `.hscroll` element. Its bounding rect
 * already includes current scroll position, so callers must not add scrollTop
 * or subtract sticky-header height again.
 */
export function rowOffsetFromContentTopY(clientY: number, contentTop: number): number {
  return clientY - contentTop;
}

export function rowIndexAtOffset(geom: EffectiveGeometry, offsetPx: number): number {
  if (geom.rowCount === 0) return -1;
  if (offsetPx <= 0) return 0;
  if (offsetPx >= geom.totalHeight) return geom.rowCount - 1;
  const i = binarySearchLeft(geom.cumulativeOffsets, offsetPx);
  if (i < geom.cumulativeOffsets.length && geom.cumulativeOffsets[i] === offsetPx) {
    return Math.min(i, geom.rowCount - 1);
  }
  return Math.max(0, i - 1);
}

/**
 * Binary search: returns the index of the first element >= target.
 * If all elements are < target, returns arr.length.
 */
export function binarySearchLeft(arr: number[], target: number): number {
  let lo = 0;
  let hi = arr.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (arr[mid] < target) {
      lo = mid + 1;
    } else {
      hi = mid;
    }
  }
  return lo;
}
