<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { browser, pageKey } from "$lib/stores/browser.svelte";
  import {
    annotationMatchesPage,
    BLANK_BROWSER_URL,
    fromProxyUrl,
    sameBrowserUrl,
    toProxyUrl,
  } from "$lib/stores/browserUrl";
  import {
    browserEnsure,
    browserHide,
    browserReload,
    browserSendToPage,
    browserSetAnnotateMode,
    browserSetBounds,
    listenBrowserMessages,
  } from "$lib/stores/browserHost";
  import type { UiDomContext } from "$lib/types";
  import AnnotationOverlay from "./AnnotationOverlay.svelte";
  import type { AnnotationSubmission } from "./AnnotationComposer.svelte";
  import {
    composerSubmission,
    hoverTarget,
    iframeClick,
    objectField,
    reanchorUpdates,
    stringField,
    type PageRect,
  } from "$lib/browserPayload";
  import {
    dismissBrowserAnnotationComposerNow,
    registerBrowserAnnotationComposerDismiss,
  } from "$lib/stores/keyboard";
  import { overlay } from "$lib/stores/overlay.svelte";

  const activeTabIdx = $derived(app.snapshot?.active_tab ?? 0);
  const showBrowserPane = $derived(browser.open);
  const nativeWebviewVisible = $derived(showBrowserPane && !overlay.blocksNativeBrowser);

  let urlInput = $state(browser.url);
  /** True while the URL bar has focus — blocks poll-driven overwrites while typing. */
  let urlBarFocused = $state(false);
  let lastUrlSyncTab = $state(-1);
  let lastSnapUrl = $state(browser.url);
  /** Native child webview pane (transparent hole in the main UI). */
  let browserPaneEl = $state<HTMLDivElement | null>(null);
  let paneWidth = $state(0);
  let paneHeight = $state(0);

  /** Fallback iframe through `erp://` when native webview is unavailable. */
  let useProxyFallback = $state(false);
  let iframeEl = $state<HTMLIFrameElement | null>(null);
  let iframeSrc = $state(toProxyUrl(browser.url));

  let paneLoading = $state(false);
  let prefillDone = $state(false);
  /** While set, ignore stale `__er_location` from the page we are leaving. */
  let pendingNavigationUrl = $state<string | null>(null);

  async function syncPaneBounds() {
    if (!browserPaneEl || !nativeWebviewVisible || useProxyFallback) return;
    const rect = browserPaneEl.getBoundingClientRect();
    paneWidth = rect.width;
    paneHeight = rect.height;
    if (rect.width < 1 || rect.height < 1) return;
    try {
      await browserSetBounds(rect.left, rect.top, rect.width, rect.height, activeTabIdx);
    } catch {
      // Native webview not available in web preview / tests.
    }
  }

  async function openNativeBrowser(url: string) {
    if (!url.trim() || url === BLANK_BROWSER_URL) return;
    useProxyFallback = false;
    paneLoading = true;
    markWaitingForReadiness();
    try {
      await browserEnsure(url, activeTabIdx);
      await syncPaneBounds();
    } catch (err) {
      console.warn("[er] native review browser unavailable, using proxy fallback", err);
      useProxyFallback = true;
      iframeSrc = toProxyUrl(url);
    }
  }

  async function navigateBrowser(url: string) {
    if (!url.trim() || url === BLANK_BROWSER_URL) return;
    paneLoading = true;
    markWaitingForReadiness();
    if (useProxyFallback) {
      iframeSrc = toProxyUrl(url);
      return;
    }
    try {
      // browser_ensure creates the child webview if navigate runs before initial open finishes
      await browserEnsure(url, activeTabIdx);
      await syncPaneBounds();
    } catch (err) {
      console.warn("[er] native review browser navigation failed, using proxy fallback", err);
      useProxyFallback = true;
      iframeSrc = toProxyUrl(url);
    }
  }

  $effect(() => {
    if (!showBrowserPane || !prefillDone) return;
    const next = browser.url;
    if (!next.trim() || next === BLANK_BROWSER_URL) return;
    if (!useProxyFallback) return;
    const proxied = toProxyUrl(next);
    if (!sameBrowserUrl(fromProxyUrl(iframeSrc), fromProxyUrl(proxied))) {
      paneLoading = true;
      iframeSrc = proxied;
    }
  });

  $effect(() => {
    if (!nativeWebviewVisible) {
      if (!showBrowserPane) prefillDone = false;
      void browserHide(activeTabIdx);
      return;
    }
    if (prefillDone) return;
    const url = browser.url.trim();
    let cancelled = false;
    void (async () => {
      if (url && url !== BLANK_BROWSER_URL) {
        await openNativeBrowser(url);
      } else {
        urlInput = "";
        await browserHide();
      }
      if (!cancelled) prefillDone = true;
    })();
    return () => {
      cancelled = true;
    };
  });

  let loadingTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    if (!paneLoading) return;
    if (loadingTimer !== null) clearTimeout(loadingTimer);
    loadingTimer = setTimeout(() => {
      paneLoading = false;
    }, 30_000);
    return () => {
      if (loadingTimer !== null) clearTimeout(loadingTimer);
    };
  });

  let hoveredEl = $state<{
    selector: string | null;
    rect: { left: number; top: number; width: number; height: number };
    element_context?: string | null;
    dom_context?: UiDomContext | null;
  } | null>(null);

  let livePinRect = $state<{ left: number; top: number; width: number; height: number } | null>(null);
  let allPinRects = $state<Record<string, { left: number; top: number; width: number; height: number } | null>>({});

  type AnnotationReadiness = "waiting" | "ready" | "unsupported";
  let annotationReadiness = $state<AnnotationReadiness>("waiting");
  const READINESS_BADGE_CLASS: Record<AnnotationReadiness, string> = {
    ready: "text-success bg-success/15",
    unsupported: "text-error bg-error/15",
    waiting: "text-warning bg-warning/15",
  };

  /** Native child webview sits above the Svelte overlay — page script handles pointer. */
  const pageHandlesAnnotate = $derived(!useProxyFallback);

  let composerOpenInPage = $state(false);

  let readinessTimer: ReturnType<typeof setTimeout> | null = null;
  /** Avoid re-arming the readiness timer on every snapshot poll. */
  let readinessContextKey = $state<string | null>(null);

  function annotationReadinessKey(): string {
    return `${activeTabIdx}|${browser.url}|${browser.annotateMode}`;
  }

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

  async function sendToPage(payload: Record<string, unknown>): Promise<void> {
    if (useProxyFallback && iframeEl?.contentWindow) {
      try {
        iframeEl.contentWindow.postMessage(payload, "*");
      } catch {
        // ignored
      }
      return;
    }
    try {
      await browserSendToPage(payload, activeTabIdx);
    } catch (err) {
      console.warn("[er] browserSendToPage failed", err);
    }
  }

  function currentPageUrl(): string {
    return browser.url.trim() || urlInput.trim();
  }

  async function clearInPageAnnotationUi() {
    closePageComposer();
    clearHoverState();
    allPinRects = {};
    if (!showBrowserPane) return;
    await sendToPage({ __er_clear_pins: true });
    await sendToPage({ __er_sync_pins: true, items: [] });
  }

  function queryHoverAt(x: number, y: number) {
    void sendToPage({ __er_hover: true, x, y });
  }

  function syncPinsToPage() {
    if (composerOpenInPage) return;
    if (!showBrowserPane || !browser.url.trim() || browser.url === BLANK_BROWSER_URL) {
      void sendToPage({ __er_clear_pins: true });
      return;
    }
    if (!pageHandlesAnnotate) {
      void sendToPage({ __er_clear_pins: true });
      return;
    }
    const items = (app.snapshot?.ui_annotations ?? [])
      .filter((a) => annotationMatchesPage(a.url, browser.url))
      .map((a, i) => ({
        id: a.id,
        selector: a.selector ?? null,
        box: [a.box_x, a.box_y, a.box_w, a.box_h],
        viewport: [a.viewport_w, a.viewport_h],
        text: a.text,
        label: a.element_context ?? a.selector ?? null,
        stale: a.stale,
        showTip: browser.showAnnotationTooltips,
        index: i + 1,
      }));
    void sendToPage({ __er_sync_pins: true, items });
  }

  const SYNC_PINS_DEBOUNCE_MS = 120;
  let syncPinsTimer: ReturnType<typeof setTimeout> | null = null;

  /** Debounced pin sync for poll/resize-driven updates; imperative syncPinsToPage() stays immediate. */
  function scheduleSyncPins() {
    if (syncPinsTimer !== null) clearTimeout(syncPinsTimer);
    syncPinsTimer = setTimeout(() => {
      syncPinsTimer = null;
      syncPinsToPage();
    }, SYNC_PINS_DEBOUNCE_MS);
  }

  function showSavedPopover(
    bbox: [number, number, number, number],
    text: string,
    elementContext: string | null,
    selector: string | null = null,
  ) {
    if (!pageHandlesAnnotate) return;
    void sendToPage({
      __er_show_popover: true,
      box: bbox,
      viewport: [Math.round(paneWidth) || 1280, Math.round(paneHeight) || 800],
      selector,
      text,
      element_context: elementContext,
      label: elementContext,
    });
  }


  function openPageComposer(p: {
    x: number;
    y: number;
    w: number;
    h: number;
    selector: string | null;
    element_context: string | null;
    dom_context: UiDomContext | null;
  }) {
    composerOpenInPage = true;
    void sendToPage({
      __er_show_composer: true,
      box: [p.x, p.y, p.w, p.h],
      viewport: [Math.round(paneWidth) || 1280, Math.round(paneHeight) || 800],
      selector: p.selector,
      element_context: p.element_context,
      label: p.element_context,
      dom_context: p.dom_context,
    });
  }

  function closePageComposer() {
    composerOpenInPage = false;
    void sendToPage({ __er_hide_composer: true });
  }

  async function syncAnnotateModeToPage() {
    if (!showBrowserPane) return;
    const active = browser.annotateMode;
    if (pageHandlesAnnotate) {
      try {
        await browserSetAnnotateMode(active, activeTabIdx);
      } catch (err) {
        console.warn("[er] browserSetAnnotateMode failed", err);
      }
      return;
    }
    await sendToPage({ __er_set_annotate_mode: active });
  }

  async function go() {
    const target = urlInput.trim();
    if (!target || target === BLANK_BROWSER_URL) return;
    paneLoading = true;
    markWaitingForReadiness();
    pendingNavigationUrl = target;
    try {
      await navigateBrowser(target);
      await browser.setUrl(target);
    } catch {
      pendingNavigationUrl = null;
    }
  }

  async function refresh() {
    const url = browser.url.trim() || urlInput.trim();
    if (!url || url === BLANK_BROWSER_URL) return;
    paneLoading = true;
    markWaitingForReadiness();
    if (useProxyFallback) {
      try {
        iframeEl?.contentWindow?.location.reload();
      } catch {
        iframeSrc = toProxyUrl(url);
      }
      return;
    }
    try {
      await browserReload(activeTabIdx);
    } catch (err) {
      console.warn("[er] browser reload failed, re-navigating", err);
      await navigateBrowser(url);
    }
  }

  function applyPageLocation(href: string) {
    const real = fromProxyUrl(href);
    if (real === "about:blank") return;
    const pending = pendingNavigationUrl;
    if (pending && !sameBrowserUrl(real, pending)) {
      return;
    }
    if (pending && sameBrowserUrl(real, pending)) {
      pendingNavigationUrl = null;
    }
    urlInput = real;
    if (real !== browser.url) {
      void browser.setUrl(real);
    }
  }

  function onUrlKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void go();
    }
  }

  function close() {
    void browser.setLayout("hidden");
    void browser.setAnnotateMode(false);
  }

  // Two-step inline confirm (native confirm() is a no-op in the Tauri webview).
  let pendingClearPage = $state(false);
  let pendingClearAll = $state(false);
  let clearPageTimer: ReturnType<typeof setTimeout> | undefined;
  let clearAllTimer: ReturnType<typeof setTimeout> | undefined;

  async function clearAnnotationsPage() {
    const all = app.snapshot?.ui_annotations ?? [];
    const onPage = all.filter((a) => annotationMatchesPage(a.url, currentPageUrl()));
    const count = onPage.length;
    if (count === 0) return;
    if (!pendingClearPage) {
      pendingClearPage = true;
      clearTimeout(clearPageTimer);
      clearPageTimer = setTimeout(() => (pendingClearPage = false), 3000);
      return;
    }
    clearTimeout(clearPageTimer);
    pendingClearPage = false;
    await clearInPageAnnotationUi();
    await app.cmd("clear_ui_annotations_for_page", { pageUrl: pageKey(currentPageUrl()) });
    syncPinsToPage();
  }

  async function clearAnnotationsAll() {
    const count = app.snapshot?.ui_annotations?.length ?? 0;
    if (count === 0) return;
    if (!pendingClearAll) {
      pendingClearAll = true;
      clearTimeout(clearAllTimer);
      clearAllTimer = setTimeout(() => (pendingClearAll = false), 3000);
      return;
    }
    clearTimeout(clearAllTimer);
    pendingClearAll = false;
    await clearInPageAnnotationUi();
    await app.cmd("clear_ui_annotations", {});
    syncPinsToPage();
  }

  function queryAllAnnotationRects() {
    const anns = (app.snapshot?.ui_annotations ?? []).filter(
      (a) => annotationMatchesPage(a.url, browser.url) && a.selector,
    );
    allPinRects = {};
    for (const a of anns) {
      void sendToPage({ __er_query_rect: true, id: a.id, selector: a.selector });
    }
  }

  function onPaneReady() {
    paneLoading = false;
    clearHoverState();
    void syncPaneBounds();
    requestReanchor();
    if (pageHandlesAnnotate) {
      syncPinsToPage();
    } else {
      queryAllAnnotationRects();
    }
  }

  function onIframeLoad() {
    onPaneReady();
  }

  function requestReanchor() {
    const items = (app.snapshot?.ui_annotations ?? [])
      .filter((a) => annotationMatchesPage(a.url, browser.url) && a.selector)
      .map((a) => ({
        id: a.id,
        selector: a.selector,
        box: [a.box_x, a.box_y, a.box_w, a.box_h],
      }));
    if (items.length === 0) return;
    void sendToPage({ __er_reanchor: true, items });
  }

  function onHoverPin(selector: string | null) {
    if (!selector) {
      livePinRect = null;
      return;
    }
    void sendToPage({ __er_query_rect: true, id: "__pin__", selector });
  }

  /** Any of these means the page script is alive and answering. */
  const READY_MARKERS = [
    "__er_hover_result",
    "__er_annotate",
    "__er_location",
    "__er_ready",
    "__er_query_rect_result",
    "__er_reanchor_result",
    "__er_annotate_mode_ack",
  ];

  function onComposerSubmit(data: Record<string, unknown>): boolean {
    if (!data.__er_composer_submit) return false;
    if (!app.canPaintOptimistic()) {
      app.explainPaintBlocked();
      return true;
    }
    composerOpenInPage = false;
    void submitAnnotation({ ...composerSubmission(data), screenshotDataUrl: null });
    return true;
  }

  /** Keyboard shortcuts the page forwards while it has focus. */
  function onPageShortcut(data: Record<string, unknown>): boolean {
    switch (stringField(data, "__er_shortcut")) {
      case "browser-cycle":
        void browser.cycleLayout();
        return true;
      case "browser-fullscreen":
        void browser.setLayout(browser.layout === "fullscreen" ? "hidden" : "fullscreen");
        return true;
      case "export-review":
        app.setMainView("export-review");
        if (browser.layout === "fullscreen") void browser.setLayout("hidden");
        return true;
      case "dismiss-overlay":
        dismissBrowserAnnotationComposerNow();
        return true;
      default:
        return false;
    }
  }

  function onQueryRectResult(data: Record<string, unknown>): boolean {
    if (!data.__er_query_rect_result) return false;
    const id = stringField(data, "id");
    const parsedRect = objectField<PageRect>(data, "rect");
    if (id === "__pin__") {
      livePinRect = parsedRect;
    } else if (id) {
      allPinRects = { ...allPinRects, [id]: parsedRect };
    }
    return true;
  }

  function onReanchorResult(data: Record<string, unknown>): boolean {
    if (!data.__er_reanchor_result) return false;
    const updates = reanchorUpdates(data);
    if (updates.length > 0) {
      void app.cmd("update_ui_annotation_anchors", { updates });
    }
    return true;
  }

  /** Checked in order; the first that returns true has handled the payload. */
  const PAYLOAD_HANDLERS: ((data: Record<string, unknown>) => boolean)[] = [
    onComposerSubmit,
    onPageShortcut,
    (data) => {
      if (!data.__er_composer_cancel) return false;
      closePageComposer();
      clearHoverState();
      return true;
    },
    (data) => {
      if (!data.__er_annotate_mode_ack) return false;
      void syncAnnotateModeToPage();
      return true;
    },
    (data) => {
      if (!data.__er_ready) return false;
      const readyHref = stringField(data, "href");
      if (readyHref) applyPageLocation(readyHref);
      void syncAnnotateModeToPage();
      return true;
    },
    (data) => {
      if (!data.__er_location) return false;
      const href = stringField(data, "href");
      if (href) applyPageLocation(href);
      return true;
    },
    onQueryRectResult,
    (data) => {
      if (!data.__er_hover_result) return false;
      if (browser.annotateMode) hoveredEl = hoverTarget(data);
      return true;
    },
    onReanchorResult,
    (data) => {
      if (data.__er_annotate && browser.annotateMode) {
        browser.pendingIframeClick = iframeClick(data);
      }
      return true;
    },
  ];

  function handleBrowserPayload(data: Record<string, unknown>) {
    if (READY_MARKERS.some((key) => key in data)) markAnnotationReady();
    for (const handle of PAYLOAD_HANDLERS) {
      if (handle(data)) return;
    }
  }

  function onWindowMessage(e: MessageEvent) {
    const data = e.data as Record<string, unknown> | null;
    if (!data || typeof data !== "object") return;
    handleBrowserPayload(data);
  }

  function submitAnnotation({
    bbox,
    selector,
    text,
    screenshotDataUrl,
    elementContext,
    domContext,
  }: AnnotationSubmission) {
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    void app.cmd("add_ui_annotation", {
      url: pageKey(browser.url),
      selector,
      bbox,
      viewport: [Math.round(paneWidth) || 1280, Math.round(paneHeight) || 800],
      text,
      screenshotDataUrl,
      elementContext,
      domContext,
    });
    showSavedPopover(bbox, text, elementContext, selector);
    syncPinsToPage();
  }

  let resizeObserver: ResizeObserver | null = null;
  let unlistenBrowser: (() => void) | null = null;

  $effect(() => {
    if (composerOpenInPage && pageHandlesAnnotate) {
      registerBrowserAnnotationComposerDismiss(() => {
        closePageComposer();
        clearHoverState();
      });
      return () => registerBrowserAnnotationComposerDismiss(null);
    }
    registerBrowserAnnotationComposerDismiss(null);
  });

  onMount(() => {
    if (browser.annotateMode) markWaitingForReadiness();
    window.addEventListener("message", onWindowMessage);
    void listenBrowserMessages((payload) => {
      handleBrowserPayload(payload);
      if ((payload as { __er_ready?: boolean }).__er_ready) {
        onPaneReady();
        markAnnotationReady();
        syncAnnotateModeToPage();
        syncPinsToPage();
      }
    }).then((fn) => {
      unlistenBrowser = fn;
    });
    if (browserPaneEl && typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => {
        void syncPaneBounds();
      });
      resizeObserver.observe(browserPaneEl);
    }
    void syncPaneBounds();
  });

  onDestroy(() => {
    window.removeEventListener("message", onWindowMessage);
    unlistenBrowser?.();
    resizeObserver?.disconnect();
    if (readinessTimer !== null) clearTimeout(readinessTimer);
    if (syncPinsTimer !== null) clearTimeout(syncPinsTimer);
    void sendToPage({ __er_clear_pins: true });
    void browserHide();
  });

  // Sync URL bar from tab state when switching tabs or when committed URL changes
  // (Go, in-page navigation) — never while the user is typing (snapshot polls).
  $effect(() => {
    const tab = activeTabIdx;
    const snapUrl = browser.url;
    if (tab !== lastUrlSyncTab) {
      lastUrlSyncTab = tab;
      lastSnapUrl = snapUrl;
      urlInput = snapUrl;
      return;
    }
    if (urlBarFocused) return;
    if (snapUrl !== lastSnapUrl) {
      lastSnapUrl = snapUrl;
      urlInput = snapUrl;
    }
  });

  $effect(() => {
    if (!browser.annotateMode) {
      readinessContextKey = null;
      clearHoverState();
      closePageComposer();
      syncAnnotateModeToPage();
      return;
    }
    if (showBrowserPane) {
      const key = annotationReadinessKey();
      if (readinessContextKey !== key) {
        readinessContextKey = key;
        if (annotationReadiness !== "ready") {
          markWaitingForReadiness();
        }
      }
      queryAllAnnotationRects();
      syncAnnotateModeToPage();
    }
  });

  $effect(() => {
    if (!pageHandlesAnnotate) return;
    const p = browser.pendingIframeClick;
    if (!p || !browser.annotateMode || composerOpenInPage) return;
    const hovered = hoveredEl;
    const rect = hovered?.rect;
    openPageComposer(
      rect && hovered
        ? {
            x: rect.left,
            y: rect.top,
            w: rect.width,
            h: rect.height,
            selector: p.selector ?? hovered.selector,
            element_context: p.element_context ?? hovered.element_context ?? null,
            dom_context: p.dom_context ?? hovered.dom_context ?? null,
          }
        : {
            x: p.x,
            y: p.y,
            w: p.w || 24,
            h: p.h || 24,
            selector: p.selector,
            element_context: p.element_context ?? null,
            dom_context: p.dom_context ?? null,
          },
    );
    browser.pendingIframeClick = null;
    clearHoverState();
  });

  $effect(() => {
    void useProxyFallback;
    syncAnnotateModeToPage();
  });

  // Single debounced path for poll/resize-driven pin sync (avoids triple IPC per snapshot).
  $effect(() => {
    if (!showBrowserPane) return;
    void activeTabIdx;
    void pageHandlesAnnotate;
    void browser.url;
    void browser.showAnnotationTooltips;
    void app.snapshot?.ui_annotations?.length;
    void paneWidth;
    void paneHeight;
    if (pageHandlesAnnotate) {
      scheduleSyncPins();
    } else {
      queryAllAnnotationRects();
    }
    return () => {
      if (syncPinsTimer !== null) {
        clearTimeout(syncPinsTimer);
        syncPinsTimer = null;
      }
    };
  });

  $effect(() => {
    void nativeWebviewVisible;
    void activeTabIdx;
    void syncPaneBounds();
  });

  $effect(() => {
    void activeTabIdx;
    void browser.url;
    if (showBrowserPane) void syncAnnotateModeToPage();
  });
</script>

<div
  class="flex flex-col h-full w-full bg-surface"
  role="region"
  aria-label="Browser view"
>
  <div class="flex items-center gap-2 px-3 py-2 border-b border-hairline shrink-0">
    <input
      bind:value={urlInput}
      onfocus={() => { urlBarFocused = true; }}
      onblur={() => { urlBarFocused = false; }}
      onkeydown={onUrlKeydown}
      class="flex-1 bg-bg border border-hairline rounded px-2 py-1 text-sm outline-none mono"
      placeholder="http://localhost:5173"
      title="Use localhost consistently — cookies differ from 127.0.0.1"
    />
    <button
      type="button"
      class="text-xs px-2 py-1 rounded bg-hover hover:opacity-80"
      onclick={() => void go()}
    >
      Go
    </button>
    <button
      type="button"
      class="text-xs px-2 py-1 rounded bg-hover hover:opacity-80"
      onclick={() => void refresh()}
      title="Reload page"
    >
      Refresh
    </button>
    {#if browser.annotateMode}
      <span
        class="text-[10px] px-1.5 py-0.5 rounded font-mono {READINESS_BADGE_CLASS[annotationReadiness]}"
      >
        {annotationReadiness === 'ready' ? 'annotation ready' : annotationReadiness}
      </span>
    {/if}
    <button
      type="button"
      class="text-xs px-2 py-1 rounded {browser.annotateMode ? 'bg-accent text-on-accent' : 'bg-hover'}"
      onclick={() => void browser.setAnnotateMode(!browser.annotateMode)}
      title="Click elements on the page to leave an annotation"
    >
      {browser.annotateMode ? "Annotating…" : "Annotate"}
    </button>
    <button
      type="button"
      class="text-xs px-2 py-1 rounded {browser.showAnnotationTooltips ? 'bg-hover text-fg' : 'hover:bg-hover text-muted'}"
      onclick={() => void browser.setShowAnnotationTooltips(!browser.showAnnotationTooltips)}
      title="Show note bubbles for all visible annotations"
      aria-pressed={browser.showAnnotationTooltips}
    >
      Tips
    </button>
    <button
      type="button"
      class="text-xs px-2 py-1 rounded disabled:opacity-40 {pendingClearPage ? 'bg-error/15 text-error' : 'hover:bg-error/15 text-muted hover:text-error'}"
      onclick={clearAnnotationsPage}
      disabled={!(app.snapshot?.ui_annotations ?? []).some((a) =>
        annotationMatchesPage(a.url, currentPageUrl()),
      )}
      title={pendingClearPage ? "Click again to confirm" : "Clear annotations on this page"}
    >
      {pendingClearPage ? "Confirm clear?" : "Clear page"}
    </button>
    <button
      type="button"
      class="text-xs px-2 py-1 rounded disabled:opacity-40 {pendingClearAll ? 'bg-error/15 text-error' : 'hover:bg-error/15 text-muted hover:text-error'}"
      onclick={clearAnnotationsAll}
      disabled={(app.snapshot?.ui_annotations?.length ?? 0) === 0}
      title={pendingClearAll ? "Click again to confirm" : "Clear all UI annotations on this review tab"}
    >
      {pendingClearAll ? "Confirm clear all?" : "Clear all"}
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

  <div
    bind:this={browserPaneEl}
    class="relative flex-1 overflow-hidden bg-transparent pointer-events-none"
  >
    {#if paneLoading && browser.url.trim() && browser.url !== BLANK_BROWSER_URL}
      <div
        class="absolute inset-0 z-10 flex items-center justify-center bg-surface/80 text-sm text-muted pointer-events-none"
        aria-live="polite"
      >
        Loading…
      </div>
    {/if}

    {#if annotationReadiness === "unsupported" && browser.annotateMode}
      <div
        class="absolute top-2 left-2 right-2 z-20 rounded border border-warning/50 bg-[color-mix(in_srgb,var(--color-warning)_15%,var(--color-bg))] px-3 py-2 text-xs text-warning pointer-events-auto"
        role="status"
      >
        Annotations need the embedded browser — reload this page or restart Easy Review.
        {#if useProxyFallback}
          <span class="block mt-1 text-warning/80">Using proxy fallback; native webview unavailable.</span>
        {/if}
      </div>
    {/if}

    {#if useProxyFallback}
      <iframe
        bind:this={iframeEl}
        src={iframeSrc}
        title="Embedded browser (proxy)"
        class="absolute inset-0 w-full h-full border-0 bg-white pointer-events-auto"
        onload={onIframeLoad}
      ></iframe>
    {/if}

    <div class="absolute inset-0 z-30 pointer-events-none">
      <AnnotationOverlay
        width={paneWidth}
        height={paneHeight}
        {pageHandlesAnnotate}
        {hoveredEl}
        {livePinRect}
        {allPinRects}
        {onHoverPin}
        {queryHoverAt}
        onPointerLeave={clearHoverState}
        getIframeRect={() => browserPaneEl?.getBoundingClientRect() ?? null}
        onSubmit={submitAnnotation}
      />
    </div>
  </div>
</div>
