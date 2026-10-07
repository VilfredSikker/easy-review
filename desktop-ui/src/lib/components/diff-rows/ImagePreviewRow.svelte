<script lang="ts">
  import { untrack } from "svelte";
  import { imagePreview } from "$lib/stores/imagePreview.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { imagePanes, stepPane, type ImagePreviewState } from "$lib/imagePreview";
  import type { AppSnapshot, FileSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
    file: FileSnapshot;
    height: number;
  }
  const { snapshot, file, height }: Props = $props();
  // A mounted row owns its result, so evicting the shared cache cannot blank it.
  // The row identity carries the preview key, so new content remounts the row.
  let previewState = $state<ImagePreviewState>({ status: "loading" });
  let retry = $state(0);
  let lastRetry = 0;
  const requestKey = $derived(imagePreview.requestKey(snapshot, file));
  $effect(() => {
    requestKey;
    const [current, image] = untrack(() => [snapshot, file] as const);
    const force = retry !== lastRetry;
    lastRetry = retry;
    let active = true;
    // A context change re-checks the cache; keep the image up while it does.
    if (force || untrack(() => previewState.status) !== "ready") previewState = { status: "loading" };
    void untrack(() => imagePreview.load(current, image, force)).then((result) => {
      if (active && result) previewState = result;
    });
    return () => { active = false; };
  });

  const panes = $derived(previewState.status === "ready" ? imagePanes(previewState, file.status) : []);
  let sizes = $state<Record<string, string>>({});
  function measure(label: string, event: Event) {
    const img = event.currentTarget as HTMLImageElement;
    // A viewBox-only SVG has no intrinsic pixel size.
    if (img.naturalWidth === 0) return;
    sizes = { ...sizes, [label]: `${img.naturalWidth} × ${img.naturalHeight}` };
  }

  let zoomed = $state<number | null>(null);
  const zoomedPane = $derived(zoomed === null ? null : panes[zoomed] ?? null);
  // A reload that drops the zoomed pane closes the view; clear the index too,
  // or the pane coming back later reopens it unasked.
  $effect(() => {
    if (zoomed !== null && zoomedPane === null) zoomed = null;
  });
  function zoomKeydown(e: KeyboardEvent) {
    if (zoomed === null || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    e.preventDefault();
    e.stopPropagation();
    zoomed = stepPane(zoomed, e.key, panes.length);
  }
</script>

<div class="image-preview" style="height:{height}px" data-image-preview={file.path}>
  {#if previewState.status === "loading"}
    <p class="image-state" role="status">Loading image…</p>
  {:else if previewState.status === "error"}
    <div class="image-state image-failed" role="status">
      <p>Image unavailable</p>
      <button type="button" onclick={() => { retry++; }}>Retry</button>
      <p class="image-error">{previewState.message}</p>
    </div>
  {:else if panes.length === 0}
    <p class="image-state">No image to show</p>
  {:else}
    {#each panes as pane, i (pane.label)}
      <figure class="image-pane">
        <figcaption>
          <span>{pane.label}{#if pane.note} · {pane.note}{/if}</span>
          {#if sizes[pane.label]}<span class="image-size">{sizes[pane.label]}</span>{/if}
        </figcaption>
        <button type="button" class="image-box" title="View full screen" onclick={() => { zoomed = i; }}>
          <!-- <img> keeps SVG scripts from running; never inline or embed it. -->
          <img src={pane.src} alt="{pane.label}: {file.path}" onload={(e) => measure(pane.label, e)} />
        </button>
      </figure>
    {/each}
  {/if}
</div>

<ModalShell
  open={zoomedPane !== null}
  ariaLabel="{zoomedPane?.label ?? 'Image'}: {file.path}"
  onClose={() => { zoomed = null; }}
  onKeydown={zoomKeydown}
  backdropClass="fixed inset-0 z-[250] bg-bg/90"
  panelClass="inset-0 flex flex-col gap-2 p-6"
>
  {#if zoomedPane}
    <div class="zoom-bar">
      <span>{file.path} · {zoomedPane.label}{#if zoomedPane.note} · {zoomedPane.note}{/if}</span>
      <span class="image-size">
        {sizes[zoomedPane.label] ?? ""}{#if panes.length > 1} · ←/→ switch{/if} · Esc close
      </span>
    </div>
    <!-- A click anywhere closes; the image is the whole point, so it gets no chrome to aim at. -->
    <button type="button" class="zoom-box" aria-label="Close full screen" onclick={() => { zoomed = null; }}>
      <img src={zoomedPane.src} alt="{zoomedPane.label}: {file.path}" />
    </button>
  {/if}
</ModalShell>

<style>
  .image-preview {
    box-sizing: border-box;
    display: flex;
    gap: 12px;
    min-width: 0;
    width: 100%;
    padding: 12px 16px;
    overflow: hidden;
  }
  .image-state { color: var(--color-fg-3); margin: 0; font-size: 0.8125rem; }
  .image-state p { margin: 0 0 0.4rem; }
  /* Retry comes first and the message scrolls, so the row stays at its fixed height. */
  .image-failed { flex: 1 1 0; display: flex; flex-direction: column; align-items: flex-start; min-height: 0; min-width: 0; }
  .image-failed .image-error { flex: 1 1 auto; min-height: 0; margin: 0.4rem 0 0; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  button { padding: 0.25rem 0.6rem; border: 1px solid var(--color-border); border-radius: 4px; color: var(--color-fg); cursor: pointer; }
  .image-pane {
    display: flex;
    flex: 1 1 0;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    margin: 0;
  }
  figcaption {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 0.75rem;
    color: var(--color-muted);
  }
  .image-size { font-family: "JetBrains Mono", monospace; font-variant-numeric: tabular-nums; }
  /* Checkerboard so transparent pixels read as transparent on any theme. */
  .image-box {
    flex: 1 1 0;
    position: relative;
    min-height: 0;
    border: 1px solid var(--color-hairline);
    border-radius: 4px;
    background: repeating-conic-gradient(var(--color-card) 0 25%, var(--color-hover) 0 50%) 0 0 / 16px 16px;
  }
  .image-box, .zoom-box { display: block; width: 100%; padding: 0; cursor: zoom-in; }
  .image-box:hover { border-color: var(--color-border); }
  .zoom-bar { display: flex; justify-content: space-between; gap: 12px; font-size: 0.75rem; color: var(--color-muted); }
  .zoom-box {
    flex: 1 1 0;
    position: relative;
    min-height: 0;
    border: 0;
    cursor: zoom-out;
    background: repeating-conic-gradient(var(--color-card) 0 25%, var(--color-hover) 0 50%) 0 0 / 16px 16px;
  }
  /* Absolute so the percentages resolve against the box; small icons stay 1:1. */
  img { position: absolute; inset: 0; margin: auto; max-width: 100%; max-height: 100%; object-fit: contain; }
</style>
