/**
 * Shared store for continuous-scroll diff view.
 *
 * - `scrollTopByMode`: last-known scroll-top per diff mode, so switching tabs
 *   (e.g. branch → unstaged) and back restores position.
 * - `currentFilePath`: idle-debounced file path visible at the top of the flat
 *   diff view (FlatDiffView writes after 200ms scroll silence). Used by
 *   FileTree to highlight and auto-scroll-into-view the viewport file.
 *   Written at most once per 200ms scroll stop — zero writes during active
 *   scrolling preserves 60fps.
 */
type DiffMode = string;

class DiffScrollStore {
  scrollTopByMode = $state<Record<DiffMode, number>>({});
  /** File path at the top of the flat virtualizer viewport; null = unknown. */
  currentFilePath = $state<string | null>(null);

  setScrollTop(mode: DiffMode, top: number) {
    this.scrollTopByMode[mode] = top;
  }

  getScrollTop(mode: DiffMode): number {
    return this.scrollTopByMode[mode] ?? 0;
  }
}

export const diffScroll = new DiffScrollStore();
