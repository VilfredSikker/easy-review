<script lang="ts">
  import { untrack } from "svelte";
  import { imagePreview } from "$lib/stores/imagePreview.svelte";
  import { imagePanes, type ImagePreviewState } from "$lib/imagePreview";
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
    {#each panes as pane (pane.label)}
      <figure class="image-pane">
        <figcaption>
          <span>{pane.label}{#if pane.note} · {pane.note}{/if}</span>
          {#if sizes[pane.label]}<span class="image-size">{sizes[pane.label]}</span>{/if}
        </figcaption>
        <div class="image-box">
          <!-- <img> keeps SVG scripts from running; never inline or embed it. -->
          <img src={pane.src} alt="{pane.label}: {file.path}" onload={(e) => measure(pane.label, e)} />
        </div>
      </figure>
    {/each}
  {/if}
</div>

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
  /* Absolute so the percentages resolve against the box; small icons stay 1:1. */
  img { position: absolute; inset: 0; margin: auto; max-width: 100%; max-height: 100%; object-fit: contain; }
</style>
