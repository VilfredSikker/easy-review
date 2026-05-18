<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { browser, pageKey } from "$lib/stores/browser.svelte";
  import { annotationMatchesPage, fromProxyUrl, sameBrowserUrl, toProxyUrl } from "$lib/stores/browserUrl";
  import type { UiDomContext } from "$lib/types";
  import AnnotationOverlay from "./AnnotationOverlay.svelte";

  let urlInput = $state(browser.url);
  let iframeEl = $state<HTMLIFrameElement | null>(null);
  let iframeWidth = $state(0);
  let iframeHeight = $state(0);

  /**
   * The src actually used for the iframe — always through the erp:// proxy.
   * This is a committed value: it only changes when navigation is explicit
   * (Go button, Enter key, palette setUrl, default-dev prefill). Updates to
   * `browser.url` that canonicalize to the same page do NOT touch the iframe,
   * preventing a feedback loop where the iframe's own `__er_location` message
   * re-triggers a reload.
   */
  let iframeSrc = $state(toProxyUrl(browser.url));

  $effect(() => {
    const next = toProxyUrl(browser.url);
    if (!sameBrowserUrl(fromProxyUrl(iframeSrc), fromProxyUrl(next))) {
      iframeSrc = next;
    }
  });

  /** Element under the cursor in annotation mode (from content-script hover query). */
  let hoveredEl = $state<{ selector: string | null; rect: { left: number; top: number; width: number; height: number }; element_context?: string | null; dom_context?: UiDomContext | null } | null>(null);

  /** Live bounding rect for the currently-hovered annotation pin (queried from the live DOM). */
  let livePinRect = $state<{ left: number; top: number; width: number; height: number } | null>(null);

  /** Live bounding rects for all visible annotations, keyed by annotation id. */
  let allPinRects = $state<Record<string, { left: number; top: number; width: number; height: number } | null>>({});

  type AnnotationReadiness = "waiting" | "ready" | "unsupported";
  let annotationReadiness = $state<AnnotationReadiness>("waiting");
  let readinessTimer: ReturnType<typeof setTimeout> | null = null;

  function clearHoverState() {
    hoveredEl = null;
    livePinRect = null;
  }

  function markWaitingForReadiness() {
    annotationReadiness = "waiting";
    if (readinessTimer !== null) clearTimeout(readinessTimer);
    readinessTimer = setTimeout(() => {
      if (annotationReadiness === "waiting") annotationReadiness = "unsupported";
    }, 1500);
  }

  function markAnnotationReady() {
    annotationReadiness = "ready";
    if (readinessTimer !== null) {
      clearTimeout(readinessTimer);
      readinessTimer = null;
    }
  }

  /** Send an immediate hover query at the given overlay-relative coords.
   *  Called by the overlay on click so the content script result arrives quickly. */
  function queryHoverAt(x: number, y: number) {
    if (!iframeEl?.contentWindow) return;
    iframeEl.contentWindow.postMessage({ __er_hover: true, x, y }, '*');
  }

  // The annotation content script is now injected at the Tauri/WebKit level via
  // initialization_script in main.rs, which runs in all frames including cross-origin
  // iframes. No need to inject it here.

  function go() {
    browser.setUrl(urlInput);
  }

  function onUrlKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      go();
    }
  }

  function close() {
    browser.open = false;
    browser.annotateMode = false;
  }

  async function clearAnnotations() {
    const count = app.snapshot?.ui_annotations?.length ?? 0;
    if (count === 0) return;
    const ok = window.confirm(`Clear ${count} UI annotation${count === 1 ? "" : "s"} for this review?`);
    if (!ok) return;
    clearHoverState();
    allPinRects = {};
    await app.cmd("clear_ui_annotations", {});
  }

  function queryAllAnnotationRects() {
    if (!iframeEl?.contentWindow) return;
    const anns = (app.snapshot?.ui_annotations ?? []).filter((a) => annotationMatchesPage(a.url, browser.url) && a.selector);
    allPinRects = {};
    for (const a of anns) {
      try {
        iframeEl.contentWindow.postMessage({ __er_query_rect: true, id: a.id, selector: a.selector }, "*");
      } catch { /* ignored */ }
    }
  }

  function onIframeLoad() {
    if (!iframeEl) return;
    clearHoverState();
    markWaitingForReadiness();
    measureIframe();
    requestReanchor();
    queryAllAnnotationRects();
  }

  function requestReanchor() {
    if (!iframeEl?.contentWindow) return;
    const items = (app.snapshot?.ui_annotations ?? [])
      .filter((a) => annotationMatchesPage(a.url, browser.url) && a.selector)
      .map((a) => ({
        id: a.id,
        selector: a.selector,
        box: [a.box_x, a.box_y, a.box_w, a.box_h],
      }));
    if (items.length === 0) return;
    try {
      iframeEl.contentWindow.postMessage({ __er_reanchor: true, items }, "*");
    } catch {
      // Cross-origin: ignored.
    }
  }

  function measureIframe() {
    if (!iframeEl) return;
    const rect = iframeEl.getBoundingClientRect();
    iframeWidth = rect.width;
    iframeHeight = rect.height;
  }

  function queryPinRect(pinId: string, selector: string) {
    if (!iframeEl?.contentWindow) return;
    try {
      iframeEl.contentWindow.postMessage({ __er_query_rect: true, id: pinId, selector }, "*");
    } catch {
      livePinRect = null;
    }
  }

  function onHoverPin(selector: string | null) {
    if (!selector) { livePinRect = null; return; }
    // selector alone isn't enough to key the response; BrowserView tracks it via
    // the __er_query_rect_result message which carries the live rect directly.
    if (!iframeEl?.contentWindow) return;
    try {
      iframeEl.contentWindow.postMessage({ __er_query_rect: true, id: "__pin__", selector }, "*");
    } catch {
      livePinRect = null;
    }
  }

  function onWindowMessage(e: MessageEvent) {
    const data = e.data as Record<string, unknown> | null;
    if (!data || typeof data !== "object") return;

    if ("__er_hover_result" in data || "__er_annotate" in data || "__er_location" in data || "__er_ready" in data) {
      markAnnotationReady();
    }

    if ((data as { __er_ready?: boolean }).__er_ready) {
      return;
    }

    // Location change from the proxied page — keep URL bar in sync.
    if ((data as { __er_location?: boolean }).__er_location) {
      const href = typeof (data as { href?: unknown }).href === "string"
        ? (data as { href: string }).href
        : null;
      if (href) {
        const real = fromProxyUrl(href);
        if (real === "about:blank") return;
        urlInput = real;
        // Avoid feedback loop: only write back when the iframe truly moved to
        // a different page. Same-page reports (trailing-slash differences,
        // implicit root paths, scheme echoes) must not retrigger iframe src.
        if (!sameBrowserUrl(real, browser.url)) {
          browser.url = real;
        }
      }
      return;
    }

    // Live rect result for a queried selector — route by id.
    if ((data as { __er_query_rect_result?: boolean }).__er_query_rect_result) {
      const id = typeof (data as { id?: unknown }).id === "string" ? (data as { id: string }).id : null;
      const rect = (data as { rect?: unknown }).rect;
      const parsedRect = rect && typeof rect === "object"
        ? (rect as { left: number; top: number; width: number; height: number })
        : null;
      if (id === "__pin__") {
        livePinRect = parsedRect;
      } else if (id) {
        allPinRects = { ...allPinRects, [id]: parsedRect };
      }
      return;
    }

    // Hover result from content script.
    if ((data as { __er_hover_result?: boolean }).__er_hover_result) {
      if (!browser.annotateMode) return;
      const rect = (data as { rect?: unknown }).rect;
      hoveredEl = rect && typeof rect === "object"
        ? {
            selector: typeof (data as { selector?: unknown }).selector === "string"
              ? (data as { selector: string }).selector
              : null,
            rect: rect as { left: number; top: number; width: number; height: number },
            element_context: typeof (data as { element_context?: unknown }).element_context === "string"
              ? (data as { element_context: string }).element_context
              : null,
            dom_context: (data as { dom_context?: unknown }).dom_context &&
              typeof (data as { dom_context?: unknown }).dom_context === "object"
              ? (data as { dom_context: UiDomContext }).dom_context
              : null,
          }
        : null;
      return;
    }

    if ((data as { __er_reanchor_result?: boolean }).__er_reanchor_result) {
      const results = Array.isArray((data as { results?: unknown }).results)
        ? ((data as { results: unknown[] }).results as Array<{
            id: string;
            fresh: boolean;
            new_box?: [number, number, number, number];
          }>)
        : [];
      if (results.length > 0) {
        const updates = results.map((r) => ({
          id: r.id,
          fresh: !!r.fresh,
          new_box: r.new_box ?? null,
        }));
        app.cmd("update_ui_annotation_anchors", { updates });
      }
      return;
    }
    if (!(data as { __er_annotate?: boolean }).__er_annotate) return;
    if (!browser.annotateMode) return;
    browser.pendingIframeClick = {
      x: Number(data.x) || 0,
      y: Number(data.y) || 0,
      w: Number(data.w) || 0,
      h: Number(data.h) || 0,
      selector: typeof data.selector === "string" ? data.selector : null,
      element_context: typeof data.element_context === "string" ? data.element_context : null,
      dom_context: data.dom_context && typeof data.dom_context === "object"
        ? data.dom_context as UiDomContext
        : null,
    };
  }

  async function submitAnnotation(
    bbox: [number, number, number, number],
    selector: string | null,
    text: string,
    screenshotDataUrl: string | null,
    elementContext: string | null,
    domContext: UiDomContext | null,
  ) {
    await app.cmd("add_ui_annotation", {
      url: pageKey(browser.url),
      selector,
      bbox,
      viewport: [Math.round(iframeWidth) || 1280, Math.round(iframeHeight) || 800],
      text,
      screenshotDataUrl,
      elementContext,
      domContext,
    });
  }

  // Listen for cross-window messages while mounted.
  let resizeObserver: ResizeObserver | null = null;
  onMount(() => {
    window.addEventListener("message", onWindowMessage);
    if (iframeEl && typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => measureIframe());
      resizeObserver.observe(iframeEl);
    }
    measureIframe();
  });
  onDestroy(() => {
    window.removeEventListener("message", onWindowMessage);
    resizeObserver?.disconnect();
    if (readinessTimer !== null) clearTimeout(readinessTimer);
  });

  // Keep urlInput in sync when external code (palette) changes the url.
  $effect(() => {
    urlInput = browser.url;
  });

  $effect(() => {
    if (!browser.annotateMode) clearHoverState();
  });

  // Re-query all annotation rects when the annotation list changes.
  $effect(() => {
    void app.snapshot?.ui_annotations?.length;
    queryAllAnnotationRects();
  });

</script>

<div
  class="flex flex-col h-full w-full bg-surface"
  role="region"
  aria-label="Browser view"
>
    <!-- Header / URL bar -->
    <div class="flex items-center gap-2 px-3 py-2 border-b border-hairline">
      <span class="text-[11px] uppercase tracking-wider text-muted">Browser</span>
      <input
        bind:value={urlInput}
        onkeydown={onUrlKeydown}
        class="flex-1 bg-bg border border-hairline rounded px-2 py-1 text-sm outline-none mono"
        placeholder="Enter a URL"
      />
      <button
        type="button"
        class="text-xs px-2 py-1 rounded bg-hover hover:opacity-80"
        onclick={go}
      >
        Go
      </button>
      {#if browser.annotateMode}
        <span
          class="text-[10px] px-1.5 py-0.5 rounded font-mono {annotationReadiness === 'ready' ? 'text-green-400 bg-green-900/30' : annotationReadiness === 'unsupported' ? 'text-red-300 bg-red-900/30' : 'text-amber-400 bg-amber-900/30'}"
          title={`src=${iframeSrc}`}
        >
          {annotationReadiness === 'ready' ? 'annotation ready' : annotationReadiness}
        </span>
      {/if}
      <button
        type="button"
        class="text-xs px-2 py-1 rounded {browser.annotateMode ? 'bg-accent text-white' : 'bg-hover'}"
        onclick={() => browser.setAnnotateMode(!browser.annotateMode)}
        title="Click elements on the page to leave an annotation"
      >
        {browser.annotateMode ? "Annotating…" : "Annotate"}
      </button>
      <button
        type="button"
        class="text-xs px-2 py-1 rounded {browser.showAnnotationTooltips ? 'bg-hover text-fg' : 'hover:bg-hover text-muted'}"
        onclick={() => browser.setShowAnnotationTooltips(!browser.showAnnotationTooltips)}
        title="Show note bubbles for all visible annotations"
        aria-pressed={browser.showAnnotationTooltips}
      >
        Tips
      </button>
      <button
        type="button"
        class="text-xs px-2 py-1 rounded hover:bg-red-900/30 text-muted hover:text-red-300 disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-muted"
        onclick={clearAnnotations}
        disabled={(app.snapshot?.ui_annotations?.length ?? 0) === 0}
        title="Clear all UI annotations for this review"
      >
        Clear
      </button>
      <button
        type="button"
        class="text-xs px-2 py-1 rounded hover:bg-hover text-muted"
        onclick={close}
        aria-label="Close browser view"
      >
        ✕
      </button>
    </div>

    <!-- Iframe + overlay -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="relative flex-1 overflow-hidden bg-black/20"
    >
      <iframe
        bind:this={iframeEl}
        src={iframeSrc}
        title="Embedded browser"
        class="w-full h-full border-0 bg-white"
        onload={onIframeLoad}
      ></iframe>
      <AnnotationOverlay
        width={iframeWidth}
        height={iframeHeight}
        {hoveredEl}
        {livePinRect}
        {allPinRects}
        {onHoverPin}
        {queryHoverAt}
        onPointerLeave={clearHoverState}
        getIframeRect={() => iframeEl?.getBoundingClientRect() ?? null}
        onSubmit={submitAnnotation}
      />
    </div>
</div>
