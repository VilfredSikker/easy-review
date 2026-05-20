// Browser pane state — backed by the active tab in `app.snapshot.browser`.
// Annotations live in `app.snapshot.ui_annotations` (persisted via Tauri commands).

import { app } from "./app.svelte";
import { DEFAULT_DEV_URL, defaultDevUrl, pageKey, urlPath } from "./browserUrl";
import type { UiDomContext } from "$lib/types";
export { DEFAULT_DEV_URL, defaultDevUrl, pageKey, urlPath };

export type BrowserLayout = "hidden" | "split" | "fullscreen";

class BrowserStore {
  /** Whether the browser pane is visible (split or fullscreen). */
  get open(): boolean {
    return this.layout !== "hidden";
  }

  get url(): string {
    return app.snapshot?.browser?.url ?? "";
  }

  get layout(): BrowserLayout {
    const l = app.snapshot?.browser?.layout ?? "hidden";
    if (l === "split" || l === "fullscreen") return l;
    return "hidden";
  }

  get splitRatio(): number {
    return app.snapshot?.browser?.split_ratio ?? 0.45;
  }

  get annotateMode(): boolean {
    return app.snapshot?.browser?.annotate_mode ?? false;
  }

  get showAnnotationTooltips(): boolean {
    return app.snapshot?.browser?.show_tooltips ?? false;
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

  async setUrl(next: string) {
    await app.cmd("update_tab_browser", { url: next });
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
