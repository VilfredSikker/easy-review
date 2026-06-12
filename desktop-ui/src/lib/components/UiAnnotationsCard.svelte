<script module lang="ts">
  const thumbnailCache = new Map<string, string>();
  const requestedScreenshots = new Set<string>();
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { annotationMatchesPage, pageKey, urlPath } from "$lib/stores/browserUrl";
  import { dismissBrowserAnnotationComposerNow } from "$lib/stores/keyboard";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import type { UiAnnotation } from "$lib/types";

  /** Only annotations for the URL currently loaded in the review browser. */
  const annotations = $derived.by(() => {
    if (!browser.open) return [];
    const url = browser.url.trim();
    if (!url) return [];
    return (app.snapshot?.ui_annotations ?? []).filter((a) =>
      annotationMatchesPage(a.url, url),
    );
  });

  const pageLabel = $derived(urlPath(pageKey(browser.url) || browser.url));

  /** Lazy-loaded thumbnails keyed by screenshot_path. */
  let thumbs = $state<Record<string, string>>({});

  function ensureThumb(path: string | null) {
    if (!path || thumbs[path]) return;
    const cached = thumbnailCache.get(path);
    if (cached) {
      thumbs[path] = cached;
      return;
    }
    if (requestedScreenshots.has(path)) return;
    requestedScreenshots.add(path);
    invoke<string>("read_annotation_screenshot", { path })
      .then((dataUrl) => {
        if (dataUrl) {
          thumbnailCache.set(path, dataUrl);
          thumbs[path] = dataUrl;
        }
      })
      .catch(() => {});
  }

  $effect(() => {
    for (const a of annotations) ensureThumb(a.screenshot_path ?? null);
  });

  $effect(() => {
    const id = browser.scrollToId;
    if (!id) return;
    const el = document.getElementById(`ui-ann-${id}`);
    el?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    queueMicrotask(() => {
      if (browser.scrollToId === id) browser.scrollToId = null;
    });
  });

  async function focusPin(id: string, url: string) {
    if (url.startsWith("http")) {
      await browser.setUrl(url);
    } else if (url.startsWith("/")) {
      try {
        const current = new URL(pageKey(browser.url || "http://localhost:5173"));
        await browser.setUrl(`${current.protocol}//${current.host}${url}`);
      } catch {
        return;
      }
    }
    await browser.setLayout("split");
    browser.highlightPinId = id;
    browser.scrollToId = id;
  }

  async function remove(id: string) {
    if (browser.highlightPinId === id) browser.highlightPinId = null;
    if (browser.scrollToId === id) browser.scrollToId = null;
    dismissBrowserAnnotationComposerNow();
    await app.cmd("delete_ui_annotation", { id });
  }
</script>

{#if annotations.length > 0}
  <Card>
    <div id="ui-annotations-card"></div>
    <SectionLabel>UI Annotations · {annotations.length}</SectionLabel>
    {#if browser.open && pageLabel}
      <div class="text-[10px] uppercase tracking-wider text-muted mono truncate mt-1" title={browser.url}>
        {pageLabel}
      </div>
    {/if}
    <ul class="mt-2 space-y-2">
      {#each annotations as a, i (a.id)}
        <li
          id={`ui-ann-${a.id}`}
          class="rounded border border-hairline p-2 transition-colors {browser.scrollToId === a.id ? 'bg-hover' : ''}"
        >
          <button
            type="button"
            class="w-full text-left"
            onclick={() => focusPin(a.id, a.url)}
          >
            <div class="flex items-center gap-2">
              <span class="rounded-full bg-accent text-on-accent text-[10px] w-5 h-5 inline-flex items-center justify-center font-bold shrink-0">
                {i + 1}
              </span>
              {#if a.stale}
                <span class="text-[10px] text-warning">stale</span>
              {/if}
            </div>
            {#if a.selector}
              <div class="mt-1 text-[10px] text-muted mono truncate" title={a.selector}>
                {a.element_context ?? a.selector}
              </div>
            {:else if a.element_context}
              <div class="mt-1 text-[10px] text-muted truncate" title={a.element_context}>
                {a.element_context}
              </div>
            {/if}
            <div class="mt-1 text-sm text-fg-2 whitespace-pre-wrap">{a.text}</div>
            {#if a.screenshot_path}
              {@const thumb = thumbs[a.screenshot_path]}
              {#if thumb}
                <img
                  src={thumb}
                  alt="Screenshot attached to annotation"
                  class="mt-2 max-h-32 w-auto rounded border border-hairline object-contain"
                  title={a.screenshot_path}
                />
              {:else}
                <div class="mt-2 text-[10px] text-muted italic">loading screenshot…</div>
              {/if}
            {/if}
          </button>
          <div class="mt-1 flex justify-end">
            <button
              type="button"
              class="text-[11px] text-muted hover:text-fg"
              onclick={() => remove(a.id)}
              aria-label="Delete annotation"
            >
              Delete
            </button>
          </div>
        </li>
      {/each}
    </ul>
  </Card>
{/if}
