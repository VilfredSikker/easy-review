// Browser pane state — the active tab's browser, from `app.snapshot.browser`
// or the tab's `view` resource by build (`browserView`).
// Annotations live in `app.snapshot.ui_annotations` (persisted via Tauri commands).

import { app } from "./app.svelte";
import { browserView } from "./tabsResource.svelte";
import { DEFAULT_DEV_URL, defaultDevUrl, pageKey, urlPath } from "./browserUrl";
import type { UiDomContext } from "$lib/pageTypes";
export { DEFAULT_DEV_URL, defaultDevUrl, pageKey, urlPath };

export type BrowserLayout = "hidden" | "split" | "fullscreen";

class BrowserStore {
  /** Whether the browser pane is visible (split or fullscreen). */
  get open(): boolean {
    return this.layout !== "hidden";
  }

  get url(): string {
    return browserView.read().url;
  }

  get layout(): BrowserLayout {
    const l = browserView.read().layout;
    if (l === "split" || l === "fullscreen") return l;
    return "hidden";
  }

  get splitRatio(): number {
    return browserView.read().split_ratio;
  }

  get annotateMode(): boolean {
    return browserView.read().annotate_mode;
  }

  get showAnnotationTooltips(): boolean {
    return browserView.read().show_tooltips;
  }

  /** Annotation id to scroll into view in the right-panel card. */
  scrollToId = $state<string | null>(null);

  /** Pin id to flash/highlight in the iframe overlay. */
  highlightPinId = $state<string | null>(null);

  /** Click intercepted by the page content-script, awaiting composer. */
  pendingIframeClick = $state<{
    x: number;
    y: number;
    w: number;
    h: number;
    selector: string | null;
    element_context?: string | null;
    dom_context?: UiDomContext | null;
  } | null>(null);

  async setLayout(layout: BrowserLayout) {
    await app.cmd("update_tab_browser", { layout });
  }

  async cycleLayout() {
    await app.cmd("cycle_tab_browser_layout");
  }

  toggleOpen() {
    void (this.layout === "hidden" ? this.setLayout("split") : this.setLayout("hidden"));
  }

  /** Persist tab URL. Pass `navigate: true` to also load it in the review webview. */
  async setUrl(next: string, opts?: { navigate?: boolean }) {
    await app.cmd("update_tab_browser", {
      url: next,
      navigate: opts?.navigate ?? false,
    });
  }

  async setAnnotateMode(v: boolean) {
    await app.cmd("update_tab_browser", { annotate: v });
  }

  async setShowAnnotationTooltips(v: boolean) {
    await app.cmd("update_tab_browser", { tooltips: v });
  }

  async setSplitRatio(r: number) {
    await app.cmd("update_tab_browser", { splitRatio: r });
  }
}

export const browser = new BrowserStore();
