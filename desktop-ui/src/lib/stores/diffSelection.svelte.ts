/**
 * Drag-select state for the diff view, mirroring the Alpine `diffSel()`
 * helper in `mocks/01-main.html` lines 47–82.
 *
 * One global instance is shared across all diff rows so they can read the
 * same start/end/dragging state and update it together on mouse events.
 */

import { DRAG_SLOP_PX, exceededDragSlop as slopExceeded } from "$lib/dragSlop";

export type SelectionKind = "comment" | "question" | "note";
export type SelectionSide = "old" | "new" | null;

export { DRAG_SLOP_PX };

class DiffSelection {
  start = $state<number | null>(null);
  end = $state<number | null>(null);
  kind = $state<SelectionKind>("comment");
  text = $state("");
  dragging = $state(false);
  /** Path of the file the selection belongs to. Cleared when switching files. */
  file = $state<string | null>(null);
  /**
   * Which diff side the selection was captured on. Unified mode also sets
   * this so old/new rows with the same line number do not both highlight.
   */
  side = $state<SelectionSide>(null);
  /** Flat-model row index where drag started. Used by FlatDiffView for file-clamp. */
  startRowIdx = $state<number | null>(null);
  /** Source index of the file the drag started in. Used by FlatDiffView to clamp cross-file. */
  startFileIndex = $state<number | null>(null);
  anchorClientX = $state(0);
  anchorClientY = $state(0);

  /**
   * Start (or extend, when shift is held) a selection at the given line.
   * `e.preventDefault()` suppresses native text selection while dragging.
   */
  begin(
    line: number,
    shift: boolean,
    e?: MouseEvent,
    file?: string | null,
    side: SelectionSide = null,
    startRowIdx?: number | null,
  ) {
    if (e) {
      e.preventDefault();
      this.anchorClientX = e.clientX;
      this.anchorClientY = e.clientY;
    }
    if (shift && this.start !== null && this.file === file && this.side === side) {
      this.end = line;
    } else {
      this.start = line;
      this.end = line;
      this.file = file ?? this.file;
      this.side = side;
      this.startRowIdx = startRowIdx ?? null;
    }
    this.dragging = true;
  }

  /** True once the pointer has moved beyond {@link DRAG_SLOP_PX} from mousedown. */
  exceededDragSlop(e: MouseEvent): boolean {
    return slopExceeded(this.anchorClientX, this.anchorClientY, e.clientX, e.clientY);
  }

  extend(line: number, side: SelectionSide = this.side) {
    if (!this.dragging) return;
    if (side !== this.side) return;
    this.end = line;
  }

  finish() {
    this.dragging = false;
  }

  /** True when this line falls within the [start..end] selection range. */
  sel(line: number, side: SelectionSide = null): boolean {
    if (this.start === null || this.end === null) return false;
    if (side !== null && this.side !== side) return false;
    const lo = Math.min(this.start, this.end);
    const hi = Math.max(this.start, this.end);
    return line >= lo && line <= hi;
  }

  first(): number {
    return Math.min(this.start ?? 0, this.end ?? 0);
  }
  last(): number {
    return Math.max(this.start ?? 0, this.end ?? 0);
  }

  /** Stable key for one-shot composer auto-scroll. */
  selectionKey(): string | null {
    if (this.start === null || this.end === null || this.file === null) return null;
    return `${this.file}:${this.first()}-${this.last()}:${this.kind}:${this.side ?? ""}`;
  }

  /** "Line 36" or "Lines 36–41" — matches mock copy. */
  rangeLabel(): string {
    if (this.start === null) return "";
    return this.first() === this.last()
      ? `Line ${this.first()}`
      : `Lines ${this.first()}–${this.last()}`;
  }

  clear() {
    this.start = null;
    this.end = null;
    this.text = "";
    this.kind = "comment";
    this.dragging = false;
    this.side = null;
    this.startRowIdx = null;
    this.startFileIndex = null;
  }

  /** True when start/end are set (highlights during drag). */
  get hasSelection(): boolean {
    return this.start !== null;
  }

  /** True when the composer should mount (after drag completes). */
  get composerOpen(): boolean {
    return this.hasSelection && !this.dragging;
  }

  /** @deprecated Use {@link hasSelection} or {@link composerOpen}. */
  get active(): boolean {
    return this.hasSelection;
  }
}

export const diffSel = new DiffSelection();
