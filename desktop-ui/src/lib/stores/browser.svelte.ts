// Browser-view state. Holds the current URL, annotate-mode toggle, and the
// drawer-open flag. Annotations themselves live in `app.snapshot.ui_annotations`
// (single source of truth, persisted via Tauri commands).

import { DEFAULT_DEV_URL, defaultDevUrl, urlPath } from "./browserUrl";
import type { UiDomContext } from "$lib/types";
export { DEFAULT_DEV_URL, defaultDevUrl, urlPath };

class BrowserStore {
  /** Whether the BrowserView drawer is visible. */
  open = $state(false);

  /** URL loaded in the iframe. User-editable via the URL bar. Starts empty
   *  so BrowserView can prefill via `defaultDevUrl(repoRoot)` on first open. */
  url = $state<string>("");

  /** When true, clicks inside the iframe are captured as annotations. */
  annotateMode = $state(false);

  /** When true, render annotation note bubbles for every current pin. */
  showAnnotationTooltips = $state(false);

  /** Annotation id to scroll into view in the right-panel card. Cleared after read. */
  scrollToId = $state<string | null>(null);

  /** Pin id to flash/highlight in the iframe overlay. Cleared after read. */
  highlightPinId = $state<string | null>(null);

  /** Click intercepted by the iframe content-script, awaiting composer. */
  pendingIframeClick = $state<{
    x: number;
    y: number;
    w: number;
    h: number;
    selector: string | null;
    element_context?: string | null;
    dom_context?: UiDomContext | null;
  } | null>(null);

  toggleOpen() {
    this.open = !this.open;
  }

  setUrl(next: string) {
    this.url = next;
  }

  setAnnotateMode(v: boolean) {
    this.annotateMode = v;
  }

  setShowAnnotationTooltips(v: boolean) {
    this.showAnnotationTooltips = v;
  }
}

export const browser = new BrowserStore();
