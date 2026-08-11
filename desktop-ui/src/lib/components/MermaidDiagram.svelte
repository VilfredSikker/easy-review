<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { themeByName } from "$lib/themes";
  import { renderMermaid } from "$lib/mermaidClient";

  interface Props {
    /** Bare mermaid source. */
    source: string;
    /** Extra classes on the wrapper (e.g. max-height for inline previews). */
    class?: string;
  }

  const { source, class: className = "" }: Props = $props();

  let svg = $state<string | null>(null);
  let error = $state<string | null>(null);

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
</script>

<div class="mermaid-diagram overflow-auto {className}">
  {#if error}
    <div class="p-3 rounded border border-del-fg/30 bg-del-bg">
      <p class="text-[11px] font-medium text-del-fg">Diagram failed to render</p>
      <p class="text-[10px] text-fg-3 mt-1 break-words">{error}</p>
    </div>
  {:else if svg}
    <!-- SVG is produced by mermaid with securityLevel "strict" (no scripts/links). -->
    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    {@html svg}
  {:else}
    <div class="flex items-center justify-center py-8 text-fg-3">
      <svg class="animate-spin" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M21 12a9 9 0 1 1-6.219-8.56"/>
      </svg>
    </div>
  {/if}
</div>

<style>
  .mermaid-diagram :global(svg) {
    max-width: 100%;
    height: auto;
    display: block;
    margin: 0 auto;
  }
</style>
