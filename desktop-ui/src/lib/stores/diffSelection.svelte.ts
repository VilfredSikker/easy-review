/**
 * Drag-select state for the diff view, mirroring the Alpine `diffSel()`
 * helper in `mocks/01-main.html` lines 47–82.
 *
 * One global instance is shared across all diff rows so they can read the
 * same start/end/dragging state and update it together on mouse events.
 */

export type SelectionKind = "comment" | "question";
export type SelectionSide = "old" | "new" | null;

class DiffSelection {
  start = $state<number | null>(null);
  end = $state<number | null>(null);
  kind = $state<SelectionKind>("comment");
  text = $state("");
  dragging = $state(false);
  /** Path of the file the selection belongs to. Cleared when switching files. */
  file = $state<string | null>(null);
  /**
   * Which split side the selection was captured on. `null` in unified mode.
   * In split mode, only cells on this side are highlighted by `sel()`.
   */
  side = $state<SelectionSide>(null);
  /** Flat-model row index where drag started. Used by FlatDiffView for file-clamp. */
  startRowIdx = $state<number | null>(null);
  /** Source index of the file the drag started in. Used by FlatDiffView to clamp cross-file. */
  startFileIndex = $state<number | null>(null);

  /**
   * Start (or extend, when shift is held) a selection at the given line.
   * `e.preventDefault()` suppresses native text selection while dragging.
   */
  begin(line: number, shift: boolean, e?: MouseEvent, file?: string | null, side: SelectionSide = null) {
    if (e) e.preventDefault();
    if (shift && this.start !== null && this.file === file) {
      this.end = line;
    } else {
      this.start = line;
      this.end = line;
      this.file = file ?? this.file;
      this.side = side;
    }
    this.dragging = true;
  }

  extend(line: number) {
    if (!this.dragging) return;
    this.end = line;
  }

  finish() {
    this.dragging = false;
  }

  /** True when this line falls within the [start..end] selection range. */
  sel(line: number): boolean {
    if (this.start === null || this.end === null) return false;
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

  /** True when there is an active selection that should reveal the composer. */
  get active(): boolean {
    return this.start !== null;
  }
}

export const diffSel = new DiffSelection();
