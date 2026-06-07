import { describe, expect, it } from "bun:test";
import {
  binarySearchLeft,
  rowIndexAtOffset,
  rowOffsetFromContentTopY,
  rowOffsetFromViewportY,
  windowFromScroll,
  windowFromScrollVariable,
  type EffectiveGeometry,
} from "./virtualWindow";

const STICKY_HEADER_PX = 40;

describe("windowFromScroll", () => {
  it("returns empty window for empty list", () => {
    const w = windowFromScroll(0, 26, 0, 500);
    expect(w).toEqual({ start: 0, end: 0, paddingTop: 0, paddingBottom: 0 });
  });

  it("starts at 0 with overscan at top when scroll is 0", () => {
    const w = windowFromScroll(100, 26, 0, 500);
    expect(w.start).toBe(0);
    expect(w.end).toBe(25);
    expect(w.paddingTop).toBe(0);
    expect(w.paddingBottom).toBe((100 - 25) * 26);
  });

  it("applies overscan above and below visible window", () => {
    const w = windowFromScroll(100, 26, 260, 260);
    expect(w.start).toBe(5);
    expect(w.end).toBe(25);
    expect(w.paddingTop).toBe(5 * 26);
    expect(w.paddingBottom).toBe((100 - 25) * 26);
  });

  it("clamps end at totalItems on last page", () => {
    const w = windowFromScroll(20, 26, 400, 260);
    expect(w.end).toBeLessThanOrEqual(20);
    expect(w.paddingBottom).toBe(0);
  });

  it("clamps start to 0 when scroll is near top with overscan", () => {
    const w = windowFromScroll(50, 26, 52, 260);
    expect(w.start).toBe(0);
    expect(w.paddingTop).toBe(0);
  });

  it("respects custom overscan", () => {
    const w = windowFromScroll(200, 26, 260, 260, 10);
    expect(w.start).toBe(0);
    expect(w.end).toBe(30);
  });

  it("handles negative scrollTop (hunk below viewport) without negative end or oversized paddingBottom", () => {
    const total = 200;
    const w = windowFromScroll(total, 24, -5000, 800);
    expect(w.start).toBeGreaterThanOrEqual(0);
    expect(w.end).toBeGreaterThanOrEqual(w.start);
    expect(w.end).toBeLessThanOrEqual(total);
    expect(w.paddingTop).toBeGreaterThanOrEqual(0);
    expect(w.paddingBottom).toBeGreaterThanOrEqual(0);
    expect(w.paddingTop + w.paddingBottom).toBeLessThanOrEqual(total * 24);
  });
});

describe("windowFromScrollVariable (terminal-entry convention)", () => {
  // 5 rows of heights [30, 50, 20, 40, 60] → cumulativeOffsets length = 6,
  // terminal entry = totalHeight = 200.
  const cumulative = [0, 30, 80, 100, 140, 200];
  const totalHeight = 200;

  it("returns empty window when no rows (cumulativeOffsets=[0])", () => {
    const w = windowFromScrollVariable([0], 0, 0, 500);
    expect(w).toEqual({ start: 0, end: 0, paddingTop: 0, paddingBottom: 0 });
  });

  it("returns empty window for fully-empty array", () => {
    const w = windowFromScrollVariable([], 0, 0, 500);
    expect(w).toEqual({ start: 0, end: 0, paddingTop: 0, paddingBottom: 0 });
  });

  it("includes the row that straddles scrollTop", () => {
    // scrollTop=35; row 1 spans 30..80 → must be in window
    const w = windowFromScrollVariable(cumulative, totalHeight, 35, 60, 0);
    expect(w.start).toBe(1);
    // visibleBottom=95; first row with top>=95 is row 3 (top=100) → end exclusive=3
    expect(w.end).toBe(3);
    expect(w.paddingTop).toBe(cumulative[1]); // 30
    expect(w.paddingBottom).toBe(totalHeight - cumulative[3]); // 100
  });

  it("scrollTop=0 with overscan returns start=0", () => {
    const w = windowFromScrollVariable(cumulative, totalHeight, 0, 60, 5);
    expect(w.start).toBe(0);
    expect(w.paddingTop).toBe(0);
    // visibleBottom=60; first row top>=60 is row 2 (top=80) → end=2+5=7, clamp to rowCount=5
    expect(w.end).toBe(5);
    expect(w.paddingBottom).toBe(0);
  });

  it("scrollTop=totalHeight clamps end at rowCount, not rowCount+1", () => {
    const w = windowFromScrollVariable(cumulative, totalHeight, totalHeight, 60, 0);
    expect(w.end).toBeLessThanOrEqual(5);
    expect(w.paddingBottom).toBeGreaterThanOrEqual(0);
  });

  it("exact row-top boundary picks that row, not the predecessor", () => {
    // scrollTop=80 is exactly the top of row 2 → row 2 included
    const w = windowFromScrollVariable(cumulative, totalHeight, 80, 20, 0);
    expect(w.start).toBe(2);
    // visibleBottom=100 is top of row 3 → first not-visible=3 → end=3
    expect(w.end).toBe(3);
  });

  it("applies overscan in both directions", () => {
    const w = windowFromScrollVariable(cumulative, totalHeight, 80, 20, 2);
    expect(w.start).toBe(0);
    expect(w.end).toBe(5);
  });

  it("3 rows × 24px (model-shaped) — start at 0, end ≤ rowCount", () => {
    const cum = [0, 24, 48, 72];
    const w = windowFromScrollVariable(cum, 72, 0, 100, 0);
    expect(w.start).toBe(0);
    expect(w.end).toBeLessThanOrEqual(3);
  });

  it("pixel overscan expands the window beyond the visible rows (fast-scroll buffer)", () => {
    // scrollTop=100 (top of row 3), viewport=20, no row overscan, no px band:
    // only the straddling row → start=3, end=4.
    const base = windowFromScrollVariable(cumulative, totalHeight, 100, 20, 0, 0);
    expect(base.start).toBe(3);
    expect(base.end).toBe(4);

    // A 30px band each direction reaches row 1 above (top=30) and row 5 below,
    // pre-rendering ahead of the scroll so a fast flick lands on rendered rows.
    const padded = windowFromScrollVariable(cumulative, totalHeight, 100, 20, 0, 30);
    expect(padded.start).toBe(1);
    expect(padded.end).toBe(5);
    expect(padded.paddingTop).toBe(cumulative[1]); // 30
    expect(padded.paddingBottom).toBe(0); // end=rowCount → no bottom spacer
  });
});

describe("rowIndexAtOffset", () => {
  const geom: EffectiveGeometry = {
    cumulativeOffsets: [0, 30, 80, 100, 140, 200],
    totalHeight: 200,
    rowCount: 5,
  };

  it("empty geometry returns -1", () => {
    expect(
      rowIndexAtOffset({ cumulativeOffsets: [0], totalHeight: 0, rowCount: 0 }, 0),
    ).toBe(-1);
  });

  it("offset 0 returns row 0", () => {
    expect(rowIndexAtOffset(geom, 0)).toBe(0);
  });

  it("offset = totalHeight returns last row (rowCount - 1)", () => {
    expect(rowIndexAtOffset(geom, geom.totalHeight)).toBe(4);
  });

  it("exact row-top boundary returns that row", () => {
    expect(rowIndexAtOffset(geom, 80)).toBe(2);
    expect(rowIndexAtOffset(geom, 100)).toBe(3);
  });

  it("1px past row top returns same row", () => {
    expect(rowIndexAtOffset(geom, 81)).toBe(2);
  });

  it("1px before row top returns predecessor", () => {
    expect(rowIndexAtOffset(geom, 79)).toBe(1);
  });

  it("negative offset returns row 0", () => {
    expect(rowIndexAtOffset(geom, -10)).toBe(0);
  });
});

describe("rowOffsetFromViewportY", () => {
  const containerTop = 100;

  it("at scrollTopPx=0, pointer 40px below container top maps to offset 0", () => {
    const clientY = containerTop + STICKY_HEADER_PX;
    expect(
      rowOffsetFromViewportY(clientY, containerTop, 0, STICKY_HEADER_PX),
    ).toBe(0);
  });

  it("at scrollTopPx=120, pointer 40px below container top maps to offset 120, not 80", () => {
    const clientY = containerTop + STICKY_HEADER_PX;
    expect(
      rowOffsetFromViewportY(clientY, containerTop, 120, STICKY_HEADER_PX),
    ).toBe(120);
    expect(
      rowOffsetFromViewportY(clientY, containerTop, 120 - STICKY_HEADER_PX, STICKY_HEADER_PX),
    ).toBe(80);
  });

  it("pointer inside the same visual line maps to that row after scrolling", () => {
    const rowHeight = 26;
    const geom: EffectiveGeometry = {
      cumulativeOffsets: [0, rowHeight, rowHeight * 2, rowHeight * 3],
      totalHeight: rowHeight * 3,
      rowCount: 3,
    };
    const scrollTopPx = 50;
    const targetRow = 2;
    const rowMidOffset = geom.cumulativeOffsets[targetRow]! + rowHeight / 2;
    const clientY =
      containerTop + STICKY_HEADER_PX + (rowMidOffset - scrollTopPx);
    const offset = rowOffsetFromViewportY(
      clientY,
      containerTop,
      scrollTopPx,
      STICKY_HEADER_PX,
    );
    expect(rowIndexAtOffset(geom, offset)).toBe(targetRow);
  });

  it("must use scrollTopPx, not scrollTopPx minus sticky header (regression)", () => {
    const scrollTopPx = 120;
    const clientY = containerTop + STICKY_HEADER_PX + 10;
    const withScrollTop = rowOffsetFromViewportY(
      clientY,
      containerTop,
      scrollTopPx,
      STICKY_HEADER_PX,
    );
    const withRowScrollTop = rowOffsetFromViewportY(
      clientY,
      containerTop,
      scrollTopPx - STICKY_HEADER_PX,
      STICKY_HEADER_PX,
    );
    expect(withScrollTop).not.toBe(withRowScrollTop);
    expect(withScrollTop - withRowScrollTop).toBe(STICKY_HEADER_PX);
  });
});

describe("rowOffsetFromContentTopY", () => {
  it("maps the content surface top to document offset 0", () => {
    expect(rowOffsetFromContentTopY(140, 140)).toBe(0);
  });

  it("uses the rendered surface top directly when scrolled", () => {
    const rowHeight = 24;
    const geom: EffectiveGeometry = {
      cumulativeOffsets: [0, rowHeight, rowHeight * 2, rowHeight * 3],
      totalHeight: rowHeight * 3,
      rowCount: 3,
    };
    const hscrollTop = -88;
    const rowTwoMidpoint = rowHeight * 2 + rowHeight / 2;
    const clientY = hscrollTop + rowTwoMidpoint;
    const offset = rowOffsetFromContentTopY(clientY, hscrollTop);
    expect(offset).toBe(rowTwoMidpoint);
    expect(rowIndexAtOffset(geom, offset)).toBe(2);
  });

  it("preserves exact boundary semantics through rowIndexAtOffset", () => {
    const geom: EffectiveGeometry = {
      cumulativeOffsets: [0, 30, 80, 100],
      totalHeight: 100,
      rowCount: 3,
    };
    const hscrollTop = 12;
    const offset = rowOffsetFromContentTopY(hscrollTop + 80, hscrollTop);
    expect(rowIndexAtOffset(geom, offset)).toBe(2);
  });
});

describe("binarySearchLeft", () => {
  it("returns first index >= target", () => {
    expect(binarySearchLeft([0, 30, 80, 100, 140, 200], 35)).toBe(2);
    expect(binarySearchLeft([0, 30, 80, 100, 140, 200], 80)).toBe(2);
    expect(binarySearchLeft([0, 30, 80, 100, 140, 200], 0)).toBe(0);
    expect(binarySearchLeft([0, 30, 80, 100, 140, 200], 250)).toBe(6);
  });
});
