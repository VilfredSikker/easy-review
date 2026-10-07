<script lang="ts">
  import type { SideBlockSpan } from "$lib/documentSideLayout";
  import DocumentMarkdown from "./DocumentMarkdown.svelte";

  interface Props {
    /** Blocks in or near the viewport, with their top in scroll content px. */
    blocks: readonly { span: SideBlockSpan; topPx: number }[];
    onheight: (key: string, px: number) => void;
  }
  const { blocks, onheight }: Props = $props();

  function measure(node: HTMLElement, key: string) {
    let current = key;
    const ro = new ResizeObserver(() => onheight(current, node.offsetHeight));
    ro.observe(node);
    return {
      update(next: string) { current = next; onheight(current, node.offsetHeight); },
      destroy() { ro.disconnect(); },
    };
  }
</script>

<!-- Rendered beside the raw rows, outside the virtual window, so a block stays
     drawn while any of its lines is on screen. -->
<div class="document-side-column">
  {#each blocks as { span, topPx } (span.key)}
    <div class="side-block" class:changed={span.changed} data-first-line={span.block.startLine} style="top:{topPx}px" use:measure={span.key}>
      <DocumentMarkdown segments={[span.block.segment]} />
    </div>
  {/each}
</div>

<style>
  .document-side-column { position: absolute; top: 0; bottom: 0; left: 50%; right: 0; pointer-events: none; }
  .side-block { position: absolute; left: 0; right: 0; display: flow-root; padding: 0.15rem 1rem 0 1rem; border-left: 2px solid transparent; color: var(--color-fg); overflow-wrap: anywhere; pointer-events: auto; }
  .side-block.changed { border-left-color: var(--color-add-fg); }
</style>
