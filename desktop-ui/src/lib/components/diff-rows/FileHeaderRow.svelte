<script lang="ts">
  import FileHeaderContent from "./FileHeaderContent.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
    /** Pin header to diff viewport width (avoids widening the horizontal scroll band). */
    viewportWidthPx?: number;
  }
  const { row, viewportWidthPx }: Props = $props();
</script>

<div
  class="h-10 px-3 border-t border-ink-650 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0 sticky left-0 z-10 overflow-hidden"
  style:width={viewportWidthPx != null && viewportWidthPx > 0 ? `${viewportWidthPx}px` : undefined}
  style:max-width={viewportWidthPx != null && viewportWidthPx > 0 ? `${viewportWidthPx}px` : undefined}
  data-row-identity={row.identity}
>
  <FileHeaderContent
    {row}
    fallbackWidthPx={viewportWidthPx != null && viewportWidthPx > 0 ? viewportWidthPx : undefined}
  />
</div>
