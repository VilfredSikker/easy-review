import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { AppSnapshot, BrowserSnapshot } from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";

/** What the browser pane shows before a tab's browser state was ever written. */
export const NO_BROWSER: BrowserSnapshot = {
  url: "",
  layout: "hidden",
  split_ratio: 0.45,
  annotate_mode: false,
  show_tooltips: false,
};

export function viewKey(tab: TabKey): ResourceKey {
  return { kind: "view", tab };
}

interface BrowserViewDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show">;
  snapshot: () => Pick<AppSnapshot, "browser"> | null;
  /** The active tab's wire key, from the tabs adapter. */
  activeKey: () => TabKey | null;
  /**
   * Runs a show or a release after the read that asked for it, since a read
   * may happen while the page renders and a release drops a held value.
   */
  later?: (run: () => void) => void;
}

/**
 * The active tab's review browser, read from `app.snapshot.browser` with the
 * flag off and from that tab's `view` resource with it on. The read shows the
 * active tab's key and releases the one it showed before.
 */
export class BrowserViewAdapter {
  private shownTab: TabKey | null = null;
  private release: (() => void) | null = null;

  constructor(private readonly deps: BrowserViewDeps) {}

  read(): BrowserSnapshot {
    if (!this.deps.flagOn) return { ...NO_BROWSER, ...this.deps.snapshot()?.browser };
    const tab = this.deps.activeKey();
    if (tab === null) return NO_BROWSER;
    if (tab !== this.shownTab) this.follow(tab);
    const held = this.deps.store.get(viewKey(tab))?.value;
    return held?.kind === "view" ? held.browser : NO_BROWSER;
  }

  private follow(tab: TabKey): void {
    this.shownTab = tab;
    const before = this.release;
    const later = this.deps.later ?? queueMicrotask;
    this.release = null;
    later(() => {
      before?.();
      if (this.shownTab === tab && this.release === null) this.release = this.deps.store.show(viewKey(tab));
    });
  }
}
