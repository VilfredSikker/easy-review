<script lang="ts">
  import FileHeaderContent from "./FileHeaderContent.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }> | null;
    /** When true, hide the overlay (real file-header row is in viewport top band). */
    hidden?: boolean;
    /** Left inset in px. Guide mode passes the pillar rail width so the overlay
        aligns with the diff column instead of spanning across the rail lane. */
    offsetLeftPx?: number;
  }
  const { row, hidden = false, offsetLeftPx = 0 }: Props = $props();
</script>

<!-- Always in DOM so it doesn't shift .hscroll layout when toggling visibility. -->
<div
  class="sticky top-0 z-30 isolate max-w-full overflow-hidden h-10 px-3 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0 {hidden || !row
    ? 'pointer-events-none invisible'
    : 'pointer-events-auto'}"
  style={offsetLeftPx > 0
    ? `margin-left:${offsetLeftPx}px;width:calc(100% - ${offsetLeftPx}px);`
    : "width:100%;"}
>
  {#if row}
    <FileHeaderContent {row} />
  {/if}
</div>
