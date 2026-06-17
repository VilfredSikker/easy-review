<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "pillar-header" }> | null;
    /** When true, hide the overlay (the inline pillar header is in the top band). */
    hidden?: boolean;
  }
  const { row, hidden = false }: Props = $props();
  const allReviewed = $derived(!!row && row.totalCount > 0 && row.reviewedCount === row.totalCount);
</script>

<!-- Compact (40px) sticky bar; same height as the file-header overlay so the
     virtual-window top-band offset (STICKY_HEADER_PX) stays valid. -->
<div
  class="sticky top-0 z-30 isolate w-full max-w-full overflow-hidden h-10 px-4 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0 {hidden || !row
    ? 'pointer-events-none invisible'
    : 'pointer-events-auto'}"
>
  {#if row}
    {#if row.foundation}
      <span class="text-[11px] text-accent shrink-0" title="Foundation">◆</span>
    {/if}
    <span class="text-[12px] font-semibold text-fg truncate">{row.title}</span>
    {#if allReviewed}
      <span class="text-[10px] px-1.5 py-[1px] rounded-full text-add-fg shrink-0" style="background: var(--color-add-bg);">Reviewed</span>
    {:else}
      <span class="mono text-[11px] text-muted shrink-0">{row.reviewedCount}/{row.totalCount}</span>
    {/if}
  {/if}
</div>
