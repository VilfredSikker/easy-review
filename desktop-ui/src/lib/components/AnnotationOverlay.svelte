<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { annotationMatchesPage } from "$lib/stores/browserUrl";
  import type { UiAnnotation, UiDomContext } from "$lib/types";

  interface Props {
    /** Width of the iframe (used to position pins). */
    width: number;
    /** Height of the iframe. */
    height: number;
    /** Hovered DOM element info from the content script (annotation mode only). */
    hoveredEl?: { selector: string | null; rect: { left: number; top: number; width: number; height: number }; element_context?: string | null; dom_context?: UiDomContext | null } | null;
    /** Live bounding rect from a DOM query for the currently-hovered annotation pin. */
    livePinRect?: { left: number; top: number; width: number; height: number } | null;
    /** Live bounding rects for all visible annotations, keyed by annotation id. */
    allPinRects?: Record<string, { left: number; top: number; width: number; height: number } | null>;
    /** Called when the user hovers a pin; parent queries the live DOM for its rect. */
    onHoverPin?: (selector: string | null) => void;
    /** Called on click to fire an immediate hover query at the given coords. */
    queryHoverAt?: (x: number, y: number) => void;
    /** Called when overlay-owned hover state should be cleared. */
    onPointerLeave?: () => void;
    /** Returns the iframe's bounding rect so screenshots can be cropped. */
    getIframeRect?: () => DOMRect | null;
    /** Called when the user submits a new annotation. */
    onSubmit: (
      bbox: [number, number, number, number],
      selector: string | null,
      text: string,
      screenshotDataUrl: string | null,
      elementContext: string | null,
      domContext: UiDomContext | null,
    ) => void;
  }

  const { width, height, hoveredEl = null, livePinRect = null, allPinRects = {}, onHoverPin, queryHoverAt, onPointerLeave, getIframeRect, onSubmit }: Props = $props();

  /** Feature-detect screen-capture support. Falls back gracefully on macOS
   *  Tauri webviews that don't ship the `getDisplayMedia` API. */
  const canCapture =
    typeof navigator !== "undefined" &&
    typeof navigator.mediaDevices !== "undefined" &&
    typeof navigator.mediaDevices.getDisplayMedia === "function";

  /** True while we're waiting on the user's screen-share grant + capture. */
  let capturing = $state(false);
  /** Capture errors surface as a small inline message under the composer. */
  let captureError = $state<string | null>(null);

  const annotations: UiAnnotation[] = $derived(
    (app.snapshot?.ui_annotations ?? []).filter((a) => annotationMatchesPage(a.url, browser.url)),
  );

  let composer = $state<{
    x: number;
    y: number;
    w: number;
    h: number;
    selector: string | null;
    element_context: string | null;
    dom_context: UiDomContext | null;
    text: string;
    /** Cached PNG data URL from a successful screen-capture, attached on save. */
    screenshotDataUrl: string | null;
  } | null>(null);

  let hoverRafPending = false;

  function clampRect(rect: { left: number; top: number; width: number; height: number }) {
    const left = Math.max(0, Math.min(rect.left, width));
    const top = Math.max(0, Math.min(rect.top, height));
    const right = Math.max(left, Math.min(rect.left + rect.width, width));
    const bottom = Math.max(top, Math.min(rect.top + rect.height, height));
    return { left, top, width: right - left, height: bottom - top };
  }

  function clampedLeft(x: number, boxWidth: number) {
    return Math.max(0, Math.min(x, Math.max(0, width - boxWidth)));
  }

  function clampedTop(y: number, boxHeight: number) {
    return Math.max(0, Math.min(y, Math.max(0, height - boxHeight)));
  }

  function onOverlayPointerMove(e: PointerEvent) {
    if (!browser.annotateMode || composer || !queryHoverAt) return;
    if (hoverRafPending) return;
    hoverRafPending = true;
    const target = e.currentTarget as HTMLElement;
    const clientX = e.clientX;
    const clientY = e.clientY;
    requestAnimationFrame(() => {
      hoverRafPending = false;
      if (!browser.annotateMode || composer) return;
      const rect = target.getBoundingClientRect();
      queryHoverAt(clientX - rect.left, clientY - rect.top);
    });
  }

  function onOverlayPointerLeave() {
    onPointerLeave?.();
  }

  function onOverlayClick(e: MouseEvent) {
    if (expandedPinId) { expandedPinId = null; return; }
    if (!browser.annotateMode || composer) return;
    browser.pendingIframeClick = null;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    if (hoveredEl?.rect) {
      // Content script is running and has a fresh hover result — use it directly.
      composer = {
        x: hoveredEl.rect.left,
        y: hoveredEl.rect.top,
        w: hoveredEl.rect.width,
        h: hoveredEl.rect.height,
        selector: hoveredEl.selector,
        element_context: hoveredEl.element_context ?? null,
        dom_context: hoveredEl.dom_context ?? null,
        text: "",
        screenshotDataUrl: null,
      };
      captureError = null;
      onPointerLeave?.();
    } else {
      // No hover result yet. Fire an immediate hover query and wait up to 200ms
      // for the result to arrive. If nothing arrives we fall back to approximate placement.
      queryHoverAt?.(x, y);
      pendingNativeClick = { x, y };
      if (pendingNativeClickTimer !== null) clearTimeout(pendingNativeClickTimer);
      pendingNativeClickTimer = setTimeout(() => {
        pendingNativeClickTimer = null;
        if (!pendingNativeClick) return;
        const c = pendingNativeClick;
        pendingNativeClick = null;
        if (composer) return;
        composer = { x: c.x, y: c.y, w: 24, h: 24, selector: null, element_context: null, dom_context: null, text: "", screenshotDataUrl: null };
        captureError = null;
        onPointerLeave?.();
      }, 200);
    }
  }

  function cancelComposer() {
    composer = null;
    captureError = null;
    onPointerLeave?.();
  }

  function saveComposer() {
    if (!composer || !composer.text.trim()) {
      composer = null;
      return;
    }
    console.log('[er:annotation] saving', {
      selector: composer.selector,
      element_context: composer.element_context,
      dom_context: composer.dom_context,
      bbox: [composer.x, composer.y, composer.w, composer.h],
    });
    onSubmit(
      [composer.x, composer.y, composer.w, composer.h],
      composer.selector,
      composer.text.trim(),
      composer.screenshotDataUrl,
      composer.element_context,
      composer.dom_context,
    );
    composer = null;
    captureError = null;
    onPointerLeave?.();
  }

  /** Use the browser's screen-share API to grab one frame, encode as PNG.
   *  We grab one video frame, paint it to a canvas, and toDataURL it. The
   *  user picks which screen/window/tab to share — we can't programmatically
   *  scope to the iframe region without same-origin DOM access. */
  async function captureScreenshot() {
    if (!canCapture || !composer || capturing) return;
    capturing = true;
    captureError = null;
    let stream: MediaStream | null = null;
    try {
      stream = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        audio: false,
      });
      const track = stream.getVideoTracks()[0];
      if (!track) throw new Error("No video track available");

      // Render one frame onto a canvas, then revoke the stream immediately.
      const video = document.createElement("video");
      video.srcObject = stream;
      video.muted = true;
      await video.play();
      // Wait one rAF so the first frame is decoded.
      await new Promise<void>((r) => requestAnimationFrame(() => r()));

      const captureW = video.videoWidth || 1280;
      const captureH = video.videoHeight || 800;

      // Attempt to crop to the iframe region. The captured surface is the
      // app window; the iframe rect (in CSS px) maps to capture pixels via
      // (captureW / innerWidth). Falls back to full frame if rect unavailable.
      const iframeRect = getIframeRect?.();
      let sx = 0, sy = 0, sw = captureW, sh = captureH;
      if (iframeRect && window.innerWidth > 0 && window.innerHeight > 0) {
        const scaleX = captureW / window.innerWidth;
        const scaleY = captureH / window.innerHeight;
        sx = Math.round(iframeRect.left * scaleX);
        sy = Math.round(iframeRect.top * scaleY);
        sw = Math.round(iframeRect.width * scaleX);
        sh = Math.round(iframeRect.height * scaleY);
        // Clamp to capture bounds.
        sx = Math.max(0, Math.min(sx, captureW - 1));
        sy = Math.max(0, Math.min(sy, captureH - 1));
        sw = Math.max(1, Math.min(sw, captureW - sx));
        sh = Math.max(1, Math.min(sh, captureH - sy));
      }

      const canvas = document.createElement("canvas");
      canvas.width = sw;
      canvas.height = sh;
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("Canvas 2D context unavailable");
      ctx.drawImage(video, sx, sy, sw, sh, 0, 0, sw, sh);

      const dataUrl = canvas.toDataURL("image/png");
      if (composer) composer.screenshotDataUrl = dataUrl;
    } catch (err) {
      captureError =
        err instanceof Error ? err.message : "Screen capture failed";
    } finally {
      if (stream) {
        for (const t of stream.getTracks()) t.stop();
      }
      capturing = false;
    }
  }

  function clearScreenshot() {
    if (composer) composer.screenshotDataUrl = null;
  }

  // Pick up clicks reported via the iframe content-script.
  // Prefer hoveredEl (more accurate bbox from hover tracking) over the click event coords.
  $effect(() => {
    const p = browser.pendingIframeClick;
    if (!p || !browser.annotateMode || composer) return;
    composer = hoveredEl?.rect
      ? {
          x: hoveredEl.rect.left,
          y: hoveredEl.rect.top,
          w: hoveredEl.rect.width,
          h: hoveredEl.rect.height,
          selector: p.selector ?? hoveredEl.selector,
          element_context: p.element_context ?? hoveredEl.element_context ?? null,
          dom_context: p.dom_context ?? hoveredEl.dom_context ?? null,
          text: "",
          screenshotDataUrl: null,
        }
      : {
          x: p.x,
          y: p.y,
          w: p.w || 24,
          h: p.h || 24,
          selector: p.selector,
          element_context: p.element_context ?? null,
          dom_context: p.dom_context ?? null,
          text: "",
          screenshotDataUrl: null,
        };
    captureError = null;
    browser.pendingIframeClick = null;
    onPointerLeave?.();
  });

  /** Lazy thumbnails for pin hover, keyed by screenshot_path. */
  let pinThumbs = $state<Record<string, string>>({});
  const pinRequested = new Set<string>();
  function ensurePinThumb(path: string | null | undefined) {
    if (!path || pinThumbs[path] || pinRequested.has(path)) return;
    pinRequested.add(path);
    invoke<string>("read_annotation_screenshot", { path })
      .then((d) => {
        if (d) pinThumbs[path] = d;
      })
      .catch(() => {});
  }

  $effect(() => {
    for (const a of annotations) ensurePinThumb(a.screenshot_path);
  });

  function pinCenter(a: UiAnnotation): { left: number; top: number } {
    return { left: a.box_x + a.box_w / 2, top: a.box_y + a.box_h / 2 };
  }

  function onPinClick(a: UiAnnotation) {
    expandedPinId = expandedPinId === a.id ? null : a.id;
    browser.scrollToId = a.id;
  }

  function deletePin(e: MouseEvent, a: UiAnnotation) {
    e.stopPropagation();
    app.cmd("delete_ui_annotation", { id: a.id });
  }

  function annotationTooltipText(a: UiAnnotation) {
    return a.element_context ? `${a.element_context}\n${a.text}` : a.text;
  }

  let hoveredPinId = $state<string | null>(null);
  let expandedPinId = $state<string | null>(null);

  /** Coords saved from onOverlayClick when hoveredEl is null — resolved once hoveredEl arrives. */
  let pendingNativeClick = $state<{ x: number; y: number } | null>(null);
  let pendingNativeClickTimer = $state<ReturnType<typeof setTimeout> | null>(null);

  // When a hoveredEl result arrives and there's a pending native click, resolve it.
  $effect(() => {
    if (!pendingNativeClick || !hoveredEl?.rect) return;
    if (pendingNativeClickTimer !== null) { clearTimeout(pendingNativeClickTimer); pendingNativeClickTimer = null; }
    const pending = pendingNativeClick;
    pendingNativeClick = null;
    if (composer) return;
    composer = {
      x: hoveredEl.rect.left,
      y: hoveredEl.rect.top,
      w: hoveredEl.rect.width,
      h: hoveredEl.rect.height,
      selector: hoveredEl.selector,
      element_context: hoveredEl.element_context ?? null,
      dom_context: hoveredEl.dom_context ?? null,
      text: "",
      screenshotDataUrl: null,
    };
    captureError = null;
    onPointerLeave?.();
  });

  // When hovering a pin, ask the parent to query the live DOM for its current rect.
  $effect(() => {
    if (hoveredPinId) {
      const a = annotations.find((x) => x.id === hoveredPinId);
      onHoverPin?.(a?.selector ?? null);
    } else {
      onHoverPin?.(null);
    }
  });

  const hoveredBbox = $derived.by(() => {
    if (!hoveredPinId) return null;
    // Prefer live rect from DOM query; fall back to viewport-scaled stored coords.
    if (livePinRect) return livePinRect;
    const a = annotations.find((x) => x.id === hoveredPinId);
    if (!a || !a.selector || a.box_w <= 0 || a.box_h <= 0 || a.viewport_w <= 0 || a.viewport_h <= 0) return null;
    const sx = width / a.viewport_w;
    const sy = height / a.viewport_h;
    return {
      left: a.box_x * sx,
      top: a.box_y * sy,
      width: a.box_w * sx,
      height: a.box_h * sy,
    };
  });

  const hoverRect = $derived(hoveredEl?.rect ? clampRect(hoveredEl.rect) : null);
</script>

<!-- Wrapper covers the iframe; click events only when annotate-mode is on. -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="absolute inset-0"
  style:pointer-events={browser.annotateMode ? "auto" : "none"}
  style:cursor={browser.annotateMode ? "crosshair" : "default"}
  onpointermove={onOverlayPointerMove}
  onpointerleave={onOverlayPointerLeave}
  onclick={onOverlayClick}
>
  <!-- DOM element highlight while in annotation mode (from content script hover query) -->
  {#if browser.annotateMode && hoverRect && !composer}
    {@const hr = hoverRect}
    <div
      class="absolute pointer-events-none rounded-sm"
      style:left="{hr.left}px"
      style:top="{hr.top}px"
      style:width="{hr.width}px"
      style:height="{hr.height}px"
      style:border="2px solid rgb(99 179 237 / 0.9)"
      style:background="rgb(99 179 237 / 0.08)"
      style:outline="1px solid rgb(255 255 255 / 0.15)"
    ></div>
    {#if hoveredEl?.selector}
      <div
        class="absolute pointer-events-none bg-black/85 text-[10px] mono text-sky-300 px-1.5 py-0.5 rounded whitespace-nowrap max-w-xs truncate"
        style:left="{clampedLeft(hr.left, 260)}px"
        style:top="{clampedTop(hr.top - 22, 22)}px"
      >
        {hoveredEl.selector}
      </div>
    {/if}
  {/if}

  {#if hoveredBbox}
    <div
      class="absolute rounded pointer-events-none"
      style:left="{hoveredBbox.left}px"
      style:top="{hoveredBbox.top}px"
      style:width="{hoveredBbox.width}px"
      style:height="{hoveredBbox.height}px"
      style:border="2px solid rgb(249 115 22 / 0.9)"
      style:background="rgb(249 115 22 / 0.12)"
      style:box-shadow="0 0 0 1px rgb(249 115 22 / 0.3)"
    ></div>
  {/if}

  {#each annotations as a, i (a.id)}
    {@const r = allPinRects?.[a.id]}
    {#if r && r.width > 0 && r.height > 0}
      <div
        class="absolute pointer-events-none rounded-sm"
        style:left="{r.left}px"
        style:top="{r.top}px"
        style:width="{r.width}px"
        style:height="{r.height}px"
        style:border="1.5px dashed rgb(249 115 22 / 0.6)"
        style:background="rgb(249 115 22 / 0.05)"
      >
        <span class="absolute top-0 left-0 text-[9px] font-bold text-orange-400 bg-black/60 px-1 leading-4 rounded-br">
          {i + 1}
        </span>
      </div>
    {/if}
  {/each}

  {#each annotations as a, i (a.id)}
    {@const c = pinCenter(a)}
    <div
      class="group absolute -translate-x-1/2 -translate-y-1/2"
      style:left="{c.left}px"
      style:top="{c.top}px"
      style:pointer-events="auto"
    >
      <button
        type="button"
        class="rounded-full text-xs font-bold w-6 h-6 flex items-center justify-center shadow-md transition hover:scale-110 {a.stale
          ? 'bg-transparent text-amber-300 border-2 border-dashed border-amber-400 ring-1 ring-amber-200/40'
          : 'bg-orange-500 text-white ring-2 ring-white/80'}"
        style:opacity={browser.highlightPinId === a.id ? 1 : 0.9}
        title={a.stale ? `stale — ${a.text}` : a.text}
        onmouseenter={() => { hoveredPinId = a.id; }}
        onmouseleave={() => { hoveredPinId = null; }}
        onclick={(e) => {
          e.stopPropagation();
          onPinClick(a);
        }}
      >
        {i + 1}
      </button>
      {#if a.stale}
        <span
          class="absolute -top-2 left-full ml-1 text-[9px] uppercase tracking-wide text-amber-300 bg-black/70 rounded px-1 opacity-0 group-hover:opacity-100 pointer-events-none whitespace-nowrap"
        >
          stale
        </span>
      {/if}
      {#if a.screenshot_path && pinThumbs[a.screenshot_path]}
        <img
          src={pinThumbs[a.screenshot_path]}
          alt=""
          class="absolute top-6 left-1/2 -translate-x-1/2 max-h-32 w-auto rounded border border-white/30 shadow-lg opacity-0 group-hover:opacity-100 pointer-events-none transition-opacity"
        />
      {/if}
      <button
        type="button"
        class="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-black/80 text-white text-[10px] leading-none flex items-center justify-center opacity-0 group-hover:opacity-100 hover:bg-red-600"
        title="Delete annotation"
        aria-label="Delete annotation"
        onclick={(e) => deletePin(e, a)}
      >
        ×
      </button>

      {#if browser.showAnnotationTooltips && expandedPinId !== a.id}
        <div
          class="absolute left-7 -top-1 w-64 max-w-[min(16rem,calc(100vw-2rem))] bg-black/85 text-white border border-white/20 rounded-md shadow-lg px-2 py-1.5 pointer-events-none z-10"
          style:left="{clampedLeft(c.left + 14, 256) - c.left}px"
          style:top="{clampedTop(c.top - 4, 84) - c.top}px"
        >
          <div class="text-[10px] text-orange-200 mono truncate">
            {a.element_context ?? a.selector ?? "Approximate location"}
          </div>
          <div class="mt-0.5 text-xs leading-snug line-clamp-3 whitespace-pre-wrap" title={annotationTooltipText(a)}>
            {a.text}
          </div>
        </div>
      {/if}

      {#if expandedPinId === a.id}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="absolute top-7 left-1/2 -translate-x-1/2 w-56 bg-card border border-border rounded-md shadow-xl p-2 z-20 pointer-events-auto"
          onclick={(e) => e.stopPropagation()}
        >
          <p class="text-xs text-fg leading-snug whitespace-pre-wrap">{a.text}</p>
          {#if a.screenshot_path && pinThumbs[a.screenshot_path]}
            <img
              src={pinThumbs[a.screenshot_path]}
              alt="Screenshot"
              class="mt-2 w-full rounded border border-hairline object-cover"
            />
          {/if}
          <div class="mt-2 flex items-center justify-between gap-2">
            <span class="text-[10px] text-muted">{a.stale ? "⚠ stale" : a.url}</span>
            <button
              type="button"
              class="text-[10px] text-muted hover:text-red-400"
              onclick={(e) => { expandedPinId = null; deletePin(e, a); }}
            >Delete</button>
          </div>
        </div>
      {/if}
    </div>
  {/each}

  {#if composer}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="absolute bg-card border border-border rounded-md shadow-lg p-2 w-64 z-10"
      style:left="{clampedLeft(composer.x, 260)}px"
      style:top="{clampedTop(composer.y + 14, 130)}px"
      style:pointer-events="auto"
      onclick={(e) => e.stopPropagation()}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <textarea
        class="w-full text-sm bg-bg border border-hairline rounded p-1 outline-none resize-none"
        rows="3"
        placeholder="What's wrong here?"
        bind:value={composer.text}
        autofocus
      ></textarea>
      {#if composer.selector}
        <div class="mt-1 text-[10px] text-muted mono truncate" title={composer.selector}>
          {composer.element_context ?? composer.selector}
        </div>
      {:else if composer.element_context}
        <div class="mt-1 text-[10px] text-muted truncate" title={composer.element_context}>
          {composer.element_context}
        </div>
      {:else}
        <div class="mt-1 text-[10px] text-muted italic">
          Approximate location
        </div>
      {/if}
      {#if composer.screenshotDataUrl}
        <div class="mt-2 flex items-center gap-2">
          <img
            src={composer.screenshotDataUrl}
            alt="Captured screenshot"
            class="h-12 w-auto rounded border border-hairline object-cover"
          />
          <button
            type="button"
            class="text-[10px] text-muted hover:text-fg underline"
            onclick={clearScreenshot}
          >
            Remove
          </button>
        </div>
      {/if}
      {#if captureError}
        <div class="mt-1 text-[10px] text-red-400" title={captureError}>
          {captureError}
        </div>
      {/if}
      <div class="flex justify-between items-center gap-2 mt-2">
        {#if canCapture}
          <button
            type="button"
            class="text-xs px-2 py-1 rounded bg-hover hover:opacity-80 text-fg disabled:opacity-50"
            onclick={captureScreenshot}
            disabled={capturing}
            title="Pick a screen/window to share; one frame is saved as PNG."
          >
            {capturing
              ? "Capturing…"
              : composer.screenshotDataUrl
                ? "Recapture"
                : "Capture screenshot"}
          </button>
        {:else}
          <span class="text-[10px] text-muted italic" title="Screen capture unavailable in this webview">
            No screen capture
          </span>
        {/if}
        <div class="flex gap-2">
          <button
            type="button"
            class="text-xs px-2 py-1 rounded hover:bg-hover text-muted"
            onclick={cancelComposer}
          >
            Cancel
          </button>
          <button
            type="button"
            class="text-xs px-2 py-1 rounded bg-accent text-white hover:opacity-90"
            onclick={saveComposer}
          >
            Save
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>
