<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { annotationMatchesPage } from "$lib/stores/browserUrl";
  import AnnotationComposer from "./AnnotationComposer.svelte";
  import type { UiAnnotation, UiDomContext } from "$lib/types";

  interface Props {
    /** Width of the iframe (used to position pins). */
    width: number;
    /** Height of the iframe. */
    height: number;
    /** Native review webview is above the overlay; the page script handles hover/click. */
    pageHandlesAnnotate?: boolean;
    /** When true, parent renders the composer in the toolbar (above the native webview). */
    composerInToolbar?: boolean;
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

  const { width, height, pageHandlesAnnotate = false, composerInToolbar = false, hoveredEl = null, livePinRect = null, allPinRects = {}, onHoverPin, queryHoverAt, onPointerLeave, getIframeRect, onSubmit }: Props = $props();

  const overlayCapturesPointer = $derived(
    browser.annotateMode && !pageHandlesAnnotate,
  );

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
        onPointerLeave?.();
      }, 200);
    }
  }

  // Pick up clicks reported via the iframe content-script.
  // Prefer hoveredEl (more accurate bbox from hover tracking) over the click event coords.
  $effect(() => {
    if (composerInToolbar) return;
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
  style:pointer-events={overlayCapturesPointer ? "auto" : "none"}
  style:cursor={overlayCapturesPointer ? "crosshair" : "default"}
  onpointermove={onOverlayPointerMove}
  onpointerleave={onOverlayPointerLeave}
  onclick={onOverlayClick}
>
  <!-- DOM element highlight while in annotation mode (from content script hover query) -->
  {#if browser.annotateMode && hoverRect && !composer && !pageHandlesAnnotate}
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

  {#if !composerInToolbar}
    <AnnotationComposer
      bind:composer
      variant="overlay"
      {width}
      {height}
      {getIframeRect}
      onSave={onSubmit}
      onCancel={onPointerLeave}
    />
  {/if}
</div>
