/**
 * Shared store for continuous-scroll diff view.
 *
 * - `currentFilePath`: the file whose section is closest to the top of the
 *   scrolling viewport (driven by an IntersectionObserver in DiffView).
 *   FileTree reads this to highlight whichever file the user is currently
 *   looking at, in addition to the selected_file cursor.
 * - `scrollTopByMode`: last-known scroll-top per diff mode, so switching tabs
 *   (e.g. branch → unstaged) and back restores position.
 */
type DiffMode = string;

class DiffScrollStore {
  currentFilePath = $state<string | null>(null);
  scrollTopByMode = $state<Record<DiffMode, number>>({});

  setScrollTop(mode: DiffMode, top: number) {
    this.scrollTopByMode[mode] = top;
  }

  getScrollTop(mode: DiffMode): number {
    return this.scrollTopByMode[mode] ?? 0;
  }
}

export const diffScroll = new DiffScrollStore();
