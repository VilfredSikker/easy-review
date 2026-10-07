<script lang="ts">
  import type { DocumentSegment } from "$lib/documentPreview";
  import { onExternalLinkClick } from "$lib/openExternalUrl";
  import MermaidDiagram from "./MermaidDiagram.svelte";

  const { segments }: { segments: readonly DocumentSegment[] } = $props();
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="document-markdown" onclick={onExternalLinkClick}>
  {#each segments as segment, index (index)}
    {#if segment.kind === "mermaid"}
      <MermaidDiagram source={segment.source} class="document-mermaid" />
    {:else}
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- HTML passes through DOMPurify and a strict document formatting allowlist. -->
      {@html segment.html}
    {/if}
  {/each}
</div>

<style>
  .document-markdown { font-size: 0.875rem; line-height: 1.6; }
  .document-markdown :global(h1), .document-markdown :global(h2), .document-markdown :global(h3),
  .document-markdown :global(h4), .document-markdown :global(h5), .document-markdown :global(h6) { font-weight: 600; line-height: 1.3; margin: 1rem 0 0.6rem; }
  .document-markdown :global(h1) { font-size: 1.6rem; }
  .document-markdown :global(h2) { font-size: 1.3rem; }
  .document-markdown :global(h3) { font-size: 1.1rem; }
  .document-markdown :global(p), .document-markdown :global(ul), .document-markdown :global(ol),
  .document-markdown :global(blockquote), .document-markdown :global(pre), .document-markdown :global(table) { margin: 0 0 0.8rem; }
  .document-markdown :global(ul) { list-style: disc; padding-left: 1.5rem; }
  .document-markdown :global(ol) { list-style: decimal; padding-left: 1.5rem; }
  .document-markdown :global(blockquote) { border-left: 3px solid var(--color-border); padding-left: 0.8rem; color: var(--color-fg-3); }
  .document-markdown :global(pre) { padding: 0.75rem; background: var(--color-card); border: 1px solid var(--color-border); border-radius: 4px; white-space: pre-wrap; }
  .document-markdown :global(code), .document-markdown :global(kbd) { font-family: "JetBrains Mono", monospace; font-size: 0.9em; }
  .document-markdown :global(table) { border-collapse: collapse; display: block; max-width: 100%; overflow-x: auto; }
  .document-markdown :global(td), .document-markdown :global(th) { border: 1px solid var(--color-border); padding: 0.3rem 0.65rem; text-align: left; }
  .document-markdown :global(th) { font-weight: 600; background: var(--color-card); }
  .document-markdown :global(a) { color: var(--color-action); text-decoration: underline; }
  .document-markdown :global(img) { max-width: 100%; height: auto; }
  .document-markdown :global(hr) { border: 0; border-top: 1px solid var(--color-border); margin: 1rem 0; }
  .document-markdown :global(summary) { cursor: pointer; }
  .document-markdown :global(.document-mermaid) { margin: 0 0 0.8rem; padding: 0.75rem; border: 1px solid var(--color-border); border-radius: 4px; }
</style>
