import { tick } from "svelte";
import type { CrossFileModel } from "$lib/diffRenderModel";
import type { FileSnapshot } from "$lib/types";

/**
 * Adapter the DiffView component registers with this store so external callers
 * (dom.ts helpers, keyboard shortcuts, FileTree) can drive scrolling without
 * importing component internals or reaching into the DOM directly.
 *
 * In legacy (non-flat) mode `getModel()` returns `null` and the store falls
 * back to `document.getElementById(...)` so the migration is non-breaking
 * before the flat virtualizer (Step C) ships.
 */
export interface DiffNavigator {
  scrollToRow(rowIdx: number, align?: "start" | "center"): void;
  scrollToEdge(to: "top" | "bottom"): void;
  requestFileContent(sourceIndex: number): Promise<void>;
  getModel(): CrossFileModel | null;
  getFiles(): FileSnapshot[];
}

function domFlash(id: string): void {
  if (typeof document === "undefined") return;
  const el = document.getElementById(id);
  if (!el) return;
  el.classList.remove("flash");
  // Force a reflow so the animation restarts when jumping to the same element twice.
  void el.offsetWidth;
  el.classList.add("flash");
  setTimeout(() => el.classList.remove("flash"), 1300);
}

class DiffNavStore {
  private nav: DiffNavigator | null = null;
  /** Set by scrollToFile/scrollToHunk to override the next snapshot-key restore. */
  pendingScrollPx: number | null = null;

  register(n: DiffNavigator): void {
    this.nav = n;
  }

  unregister(): void {
    this.nav = null;
  }

  async scrollToFile(path: string): Promise<void> {
    if (!this.nav) return;
    const model = this.nav.getModel();
    if (model) {
      const row = model.fileStartRow.get(path);
      if (row !== undefined) {
        const top = model.cumulativeOffsets[row] ?? 0;
        this.pendingScrollPx = top;
        this.nav.scrollToRow(row, "start");
        const file = this.nav.getFiles().find((f) => f.path === path);
        if (file?.is_lazy_stub) await this.nav.requestFileContent(file.source_index);
        return;
      }
    }
    // Legacy DOM fallback — also the path when no flat model exists yet.
    if (typeof document !== "undefined") {
      document
        .getElementById(`file-${path}`)
        ?.scrollIntoView({ behavior: "auto", block: "start" });
    }
  }

  async scrollToThread(threadId: string, opts: { flashId?: string } = {}): Promise<boolean> {
    const flashId = opts.flashId ?? threadId;
    if (!this.nav) {
      domFlash(flashId);
      return typeof document !== "undefined" && document.getElementById(flashId) !== null;
    }
    const model = this.nav.getModel();
    if (model) {
      const idx = model.threadRowIndex(threadId);
      if (idx !== null) {
        this.nav.scrollToRow(idx, "center");
        await tick();
        domFlash(flashId);
        return true;
      }
    }
    if (typeof document !== "undefined") {
      const el = document.getElementById(flashId);
      if (el) {
        el.scrollIntoView({ behavior: "auto", block: "center" });
        domFlash(flashId);
        return true;
      }
    }
    return false;
  }

  scrollToEdge(to: "top" | "bottom"): void {
    this.nav?.scrollToEdge(to);
  }

  /**
   * Scroll to the previous or next hunk within a file relative to the current
   * scroll position. `scrollTopPx` should be the raw scrollTop of the diff
   * container (before subtracting the sticky header offset).
   */
  scrollToAdjacentHunk(path: string, direction: "prev" | "next", scrollTopPx: number): void {
    if (!this.nav) return;
    const model = this.nav.getModel();
    if (!model) return;
    const hunkRows = model.hunkStartRow.get(path);
    if (!hunkRows || hunkRows.length === 0) return;
    const STICKY_HEADER_PX = 40;
    // Offset into the row coordinate space (same adjustment FlatDiffView uses).
    const rowTop = Math.max(0, scrollTopPx - STICKY_HEADER_PX);
    if (direction === "next") {
      // Find first hunk whose top is strictly below the current view top.
      for (const rowIdx of hunkRows) {
        const top = model.cumulativeOffsets[rowIdx] ?? 0;
        if (top > rowTop + 4) {
          this.pendingScrollPx = top;
          this.nav.scrollToRow(rowIdx, "start");
          return;
        }
      }
      // Already past last hunk — stay (or wrap to first).
    } else {
      // Find last hunk whose top is strictly above the current view top.
      let targetRowIdx: number | null = null;
      for (const rowIdx of hunkRows) {
        const top = model.cumulativeOffsets[rowIdx] ?? 0;
        if (top < rowTop - 4) targetRowIdx = rowIdx;
      }
      if (targetRowIdx !== null) {
        const top = model.cumulativeOffsets[targetRowIdx] ?? 0;
        this.pendingScrollPx = top;
        this.nav.scrollToRow(targetRowIdx, "start");
      }
    }
  }

  scrollToHunk(path: string, hunkIdx: number): void {
    if (!this.nav) return;
    const model = this.nav.getModel();
    if (!model) return;
    const hunks = model.hunkStartRow.get(path);
    if (!hunks || hunkIdx < 0 || hunkIdx >= hunks.length) {
      const fileRow = model.fileStartRow.get(path);
      if (fileRow !== undefined) {
        const top = model.cumulativeOffsets[fileRow] ?? 0;
        this.pendingScrollPx = top;
        this.nav.scrollToRow(fileRow, "start");
      }
      return;
    }
    const rowIdx = hunks[hunkIdx];
    const top = model.cumulativeOffsets[rowIdx] ?? 0;
    this.pendingScrollPx = top;
    this.nav.scrollToRow(rowIdx, "start");
  }

  async scrollToFinding(findingId: string, opts: { flashId?: string } = {}): Promise<void> {
    const flashId = opts.flashId ?? `finding-${findingId}`;
    if (!this.nav) {
      domFlash(flashId);
      return;
    }
    const model = this.nav.getModel();
    if (model) {
      const idx = model.findingRowIndex(findingId);
      if (idx !== null) {
        this.nav.scrollToRow(idx, "center");
        await tick();
        domFlash(flashId);
        return;
      }
    }
    if (typeof document !== "undefined") {
      document
        .getElementById(flashId)
        ?.scrollIntoView({ behavior: "auto", block: "center" });
    }
    domFlash(flashId);
  }
}

export const diffNav = new DiffNavStore();
