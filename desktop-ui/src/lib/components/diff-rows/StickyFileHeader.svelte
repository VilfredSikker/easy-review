<script lang="ts">
  import FileHeaderContent from "./FileHeaderContent.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }> | null;
    /** When true, hide the overlay (real file-header row is in viewport top band). */
    hidden?: boolean;
  }
  const { row, hidden = false }: Props = $props();
</script>

<!-- Always in DOM so it doesn't shift .hscroll layout when toggling visibility. -->
<div
  class="sticky top-0 z-30 isolate h-10 px-3 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0 {hidden || !row
    ? 'pointer-events-none invisible'
    : 'pointer-events-auto'}"
>
  {#if row}
    <FileHeaderContent {row} />
  {/if}
</div>
