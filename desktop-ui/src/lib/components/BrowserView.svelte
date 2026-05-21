<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
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
    browserSendToPage,
    browserSetAnnotateMode,
    browserSetBounds,
    listenBrowserMessages,
  } from "$lib/stores/browserHost";
  import type { UiDomContext } from "$lib/types";
  import AnnotationOverlay from "./AnnotationOverlay.svelte";
  import { closeAiActionPalette } from "$lib/components/AiActionPalette.svelte";
  import { openExportModal } from "$lib/components/ExportModal.svelte";
  import {
    dismissBrowserAnnotationComposerNow,
    registerBrowserAnnotationComposerDismiss,
    triggerAiPalette,
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
    if (useProxyFallback) {
      const proxied = toProxyUrl(next);
      if (!sameBrowserUrl(fromProxyUrl(iframeSrc), fromProxyUrl(proxied))) {
        paneLoading = true;
        iframeSrc = proxied;
      }
      return;
    }
    if (!sameBrowserUrl(next, urlInput)) {
      void navigateBrowser(next);
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
    void (async () => {
      if (url && url !== BLANK_BROWSER_URL) {
        await openNativeBrowser(url);
      } else {
        urlInput = "";
        await browserHide();
      }
      prefillDone = true;
    })();
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

  /** Native child webview sits above the Svelte overlay — page script handles pointer. */
  const pageHandlesAnnotate = $derived(!useProxyFallback);

  let composerOpenInPage = $state(false);

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

  function go() {
    paneLoading = true;
    markWaitingForReadiness();
    browser.setUrl(urlInput);
    void navigateBrowser(urlInput);
  }

  function onUrlKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      go();
    }
  }

  function close() {
    void browser.setLayout("hidden");
    void browser.setAnnotateMode(false);
  }

  async function openSignInHelper() {
    const url = browser.url.trim() || urlInput.trim();
    if (!url) return;
    try {
      await invoke("open_url_in_browser", { url });
    } catch {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  }

  async function clearAnnotationsPage() {
    const all = app.snapshot?.ui_annotations ?? [];
    const onPage = all.filter((a) => annotationMatchesPage(a.url, currentPageUrl()));
    const count = onPage.length;
    if (count === 0) return;
    const ok = window.confirm(
      `Clear ${count} annotation${count === 1 ? "" : "s"} on this page?`,
    );
    if (!ok) return;
    await clearInPageAnnotationUi();
    await app.cmd("clear_ui_annotations_for_page", { pageUrl: pageKey(currentPageUrl()) });
    syncPinsToPage();
  }

  async function clearAnnotationsAll() {
    const count = app.snapshot?.ui_annotations?.length ?? 0;
    if (count === 0) return;
    const ok = window.confirm(
      `Clear all ${count} UI annotation${count === 1 ? "" : "s"} on this review tab?`,
    );
    if (!ok) return;
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

  function handleBrowserPayload(data: Record<string, unknown>) {
    if (
      "__er_hover_result" in data ||
      "__er_annotate" in data ||
      "__er_location" in data ||
      "__er_ready" in data ||
      "__er_query_rect_result" in data ||
      "__er_reanchor_result" in data ||
      "__er_annotate_mode_ack" in data
    ) {
      markAnnotationReady();
    }

    if ((data as { __er_composer_submit?: boolean }).__er_composer_submit) {
      composerOpenInPage = false;
      const box = (data as { box?: number[] }).box;
      const bbox: [number, number, number, number] = Array.isArray(box) && box.length >= 4
        ? [Number(box[0]) || 0, Number(box[1]) || 0, Number(box[2]) || 24, Number(box[3]) || 24]
        : [0, 0, 24, 24];
      const text = typeof (data as { text?: unknown }).text === "string"
        ? (data as { text: string }).text
        : "";
      const selector = typeof (data as { selector?: unknown }).selector === "string"
        ? (data as { selector: string }).selector
        : null;
      const element_context = typeof (data as { element_context?: unknown }).element_context === "string"
        ? (data as { element_context: string }).element_context
        : null;
      const dom_context = (data as { dom_context?: unknown }).dom_context &&
        typeof (data as { dom_context?: unknown }).dom_context === "object"
        ? (data as { dom_context: UiDomContext }).dom_context
        : null;
      void submitAnnotation(bbox, selector, text, null, element_context, dom_context);
      return;
    }

    const shortcut = typeof (data as { __er_shortcut?: unknown }).__er_shortcut === "string"
      ? (data as { __er_shortcut: string }).__er_shortcut
      : null;
    if (shortcut === "ai-hub") {
      triggerAiPalette();
      return;
    }
    if (shortcut === "browser-cycle") {
      void browser.cycleLayout();
      return;
    }
    if (shortcut === "browser-fullscreen") {
      void browser.setLayout(browser.layout === "fullscreen" ? "hidden" : "fullscreen");
      return;
    }
    if (shortcut === "export-review") {
      openExportModal();
      return;
    }
    if (shortcut === "dismiss-overlay") {
      closeAiActionPalette();
      dismissBrowserAnnotationComposerNow();
      return;
    }

    if ((data as { __er_composer_cancel?: boolean }).__er_composer_cancel) {
      closePageComposer();
      clearHoverState();
      return;
    }

    if ((data as { __er_annotate_mode_ack?: boolean }).__er_annotate_mode_ack) {
      void syncAnnotateModeToPage();
      return;
    }

    if ((data as { __er_ready?: boolean }).__er_ready) {
      void syncAnnotateModeToPage();
      return;
    }

    if ((data as { __er_location?: boolean }).__er_location) {
      const href = typeof (data as { href?: unknown }).href === "string"
        ? (data as { href: string }).href
        : null;
      if (href) {
        const real = fromProxyUrl(href);
        if (real === "about:blank") return;
        urlInput = real;
        if (!sameBrowserUrl(real, browser.url)) {
          void browser.setUrl(real);
        }
      }
      return;
    }

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
        void app.cmd("update_ui_annotation_anchors", { updates });
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

  function onWindowMessage(e: MessageEvent) {
    const data = e.data as Record<string, unknown> | null;
    if (!data || typeof data !== "object") return;
    handleBrowserPayload(data);
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

  $effect(() => {
    void app.snapshot?.ui_annotations?.length;
    if (pageHandlesAnnotate) {
      syncPinsToPage();
    } else {
      queryAllAnnotationRects();
    }
  });

  $effect(() => {
    void pageHandlesAnnotate;
    void browser.url;
    void browser.showAnnotationTooltips;
    void app.snapshot?.ui_annotations;
    void paneWidth;
    void paneHeight;
    syncPinsToPage();
  });

  $effect(() => {
    void nativeWebviewVisible;
    void activeTabIdx;
    void syncPaneBounds();
  });

  $effect(() => {
    void activeTabIdx;
    void browser.url;
    if (showBrowserPane) {
      syncPinsToPage();
      void syncAnnotateModeToPage();
    }
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
      onclick={go}
    >
      Go
    </button>
    <button
      type="button"
      class="text-xs px-2 py-1 rounded hover:bg-hover text-muted"
      onclick={openSignInHelper}
      title="Open this URL in your system browser to sign in, then return here"
    >
      Sign in
    </button>
    {#if browser.annotateMode}
      <span
        class="text-[10px] px-1.5 py-0.5 rounded font-mono {annotationReadiness === 'ready' ? 'text-green-400 bg-green-900/30' : annotationReadiness === 'unsupported' ? 'text-red-300 bg-red-900/30' : 'text-amber-400 bg-amber-900/30'}"
      >
        {annotationReadiness === 'ready' ? 'annotation ready' : annotationReadiness}
      </span>
    {/if}
    <button
      type="button"
      class="text-xs px-2 py-1 rounded {browser.annotateMode ? 'bg-accent text-white' : 'bg-hover'}"
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
      class="text-xs px-2 py-1 rounded hover:bg-red-900/30 text-muted hover:text-red-300 disabled:opacity-40"
      onclick={clearAnnotationsPage}
      disabled={!(app.snapshot?.ui_annotations ?? []).some((a) =>
        annotationMatchesPage(a.url, currentPageUrl()),
      )}
      title="Clear annotations on this page"
    >
      Clear page
    </button>
    <button
      type="button"
      class="text-xs px-2 py-1 rounded hover:bg-red-900/30 text-muted hover:text-red-300 disabled:opacity-40"
      onclick={clearAnnotationsAll}
      disabled={(app.snapshot?.ui_annotations?.length ?? 0) === 0}
      title="Clear all UI annotations on this review tab"
    >
      Clear all
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

  <!-- svelte-ignore a11y_no_static_element_interactions -->
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
        class="absolute top-2 left-2 right-2 z-20 rounded border border-amber-700/50 bg-amber-950/90 px-3 py-2 text-xs text-amber-100 pointer-events-auto"
        role="status"
      >
        Annotations need the embedded browser — reload this page or restart Easy Review.
        {#if useProxyFallback}
          <span class="block mt-1 text-amber-200/80">Using proxy fallback; native webview unavailable.</span>
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
