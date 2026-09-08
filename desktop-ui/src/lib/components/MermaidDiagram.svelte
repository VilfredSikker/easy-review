<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { themeByName } from "$lib/themes";
  import { renderMermaid } from "$lib/mermaidClient";

  interface Props {
    /** Bare mermaid source. */
    source: string;
    /** Extra classes on the wrapper (e.g. max-height for inline previews). */
    class?: string;
    /** Enable zoom/pan controls and interactions. */
    interactive?: boolean;
  }

  const { source, class: className = "", interactive = false }: Props = $props();

  let svg = $state<string | null>(null);
  let error = $state<string | null>(null);
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let dragging = $state(false);
  let dragStart = $state({ x: 0, y: 0 });
  let panStart = $state({ x: 0, y: 0 });
  let containerEl = $state<HTMLDivElement | null>(null);

  const ZOOM_MIN = 0.5;
  const ZOOM_MAX = 3;
  const ZOOM_STEP = 0.25;

  function clampZoom(z: number): number {
    return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z));
  }

  function zoomBy(delta: number, centerX?: number, centerY?: number) {
    const prev = zoom;
    const next = clampZoom(prev + delta);
    if (next === prev) return;
    // Zoom centered on cursor position when container ref is available.
    if (centerX !== undefined && centerY !== undefined && containerEl) {
      const rect = containerEl.getBoundingClientRect();
      const cx = centerX - rect.left - rect.width / 2;
      const cy = centerY - rect.top - rect.height / 2;
      const ratio = next / prev;
      panX = cx - (cx - panX) * ratio;
      panY = cy - (cy - panY) * ratio;
    }
    zoom = next;
  }

  function resetView() {
    zoom = 1;
    panX = 0;
    panY = 0;
  }

  function onWheel(e: WheelEvent) {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      zoomBy(e.deltaY > 0 ? -ZOOM_STEP : ZOOM_STEP, e.clientX, e.clientY);
    }
  }

  function onPointerDown(e: PointerEvent) {
    if (!interactive) return;
    if (e.button !== 0) return;
    dragging = true;
    dragStart = { x: e.clientX, y: e.clientY };
    panStart = { x: panX, y: panY };
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    e.preventDefault();
    panX = panStart.x + (e.clientX - dragStart.x);
    panY = panStart.y + (e.clientY - dragStart.y);
  }

  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.target as HTMLElement).releasePointerCapture?.(e.pointerId);
  }

  // Re-render when the source or the active theme changes. Rendering is async
  // and cached by `theme::source` in the client, so remounts and theme
  // switches back to a previously-seen combination are free.
  $effect(() => {
    const theme = themeByName(app.snapshot?.theme);
    const src = source;
    let cancelled = false;
    svg = null;
    error = null;
    renderMermaid(src, theme)
      .then((rendered) => {
        if (!cancelled) svg = rendered;
      })
      .catch((e) => {
        if (!cancelled) error = e instanceof Error ? e.message : String(e);
      });
    return () => {
      cancelled = true;
    };
  });

  // Reset zoom/pan when source changes.
  $effect(() => {
    source;
    resetView();
  });

  // Svelte wheel handlers are passive — register manually so preventDefault works
  // for Ctrl/Cmd+scroll zoom. Only attached when interactive.
  $effect(() => {
    if (!interactive || !containerEl) return;
    const el = containerEl;
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      el.removeEventListener("wheel", onWheel);
    };
  });
</script>

<div
  bind:this={containerEl}
  class="mermaid-diagram {interactive ? 'overflow-hidden' : 'overflow-auto'} {className}"
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  ondblclick={interactive ? resetView : undefined}
>
  {#if error}
    <div class="p-3 rounded border border-del-fg/30 bg-del-bg">
      <p class="text-[11px] font-medium text-del-fg">Diagram failed to render</p>
      <p class="text-[10px] text-fg-3 mt-1 break-words">{error}</p>
    </div>
  {:else if svg}
    {#if interactive}
      <div
        class="w-full h-full cursor-grab"
        class:cursor-grabbing={dragging}
        style="transform: scale({zoom}) translate({panX / zoom}px, {panY / zoom}px); transform-origin: center center; will-change: transform;"
      >
        <!-- SVG is produced by mermaid with securityLevel "strict" (no scripts/links). -->
        <!-- eslint-disable-next-line svelte/no-at-html-tags -->
        {@html svg}
      </div>
    {:else}
      <!-- SVG is produced by mermaid with securityLevel "strict" (no scripts/links). -->
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      {@html svg}
    {/if}
  {:else}
    <div class="flex items-center justify-center py-8 text-fg-3">
      <svg class="animate-spin" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
      </svg>
    </div>
  {/if}
</div>

{#if interactive}
  <div class="absolute bottom-3 right-3 flex items-center gap-1 rounded-lg border border-hairline bg-surface/90 shadow-sm backdrop-blur">
    <button
      type="button"
      onclick={() => zoomBy(-ZOOM_STEP)}
      disabled={zoom <= ZOOM_MIN}
      aria-label="Zoom out"
      class="p-1.5 text-fg-2 hover:text-fg hover:bg-hover rounded-l-lg transition-colors disabled:opacity-40"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <line x1="5" y1="12" x2="19" y2="12"/>
      </svg>
    </button>
    <span class="text-[10px] font-medium text-fg-3 w-10 text-center select-none">
      {Math.round(zoom * 100)}%
    </span>
    <button
      type="button"
      onclick={() => zoomBy(ZOOM_STEP)}
      disabled={zoom >= ZOOM_MAX}
      aria-label="Zoom in"
      class="p-1.5 text-fg-2 hover:text-fg hover:bg-hover transition-colors disabled:opacity-40"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/>
      </svg>
    </button>
    <button
      type="button"
      onclick={resetView}
      aria-label="Reset zoom"
      class="p-1.5 text-fg-2 hover:text-fg hover:bg-hover rounded-r-lg transition-colors"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/>
      </svg>
    </button>
  </div>
{/if}

<style>
  .mermaid-diagram :global(svg) {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 0 auto;
  }
</style>
