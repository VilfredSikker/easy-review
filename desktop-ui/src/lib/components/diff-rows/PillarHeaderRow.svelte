<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "pillar-header" }>;
  }
  const { row }: Props = $props();

  const allReviewed = $derived(row.totalCount > 0 && row.reviewedCount === row.totalCount);

  function reviewAll() {
    if (allReviewed) void app.cmd("unbulk_review_pillar", { pillarId: row.pillarId });
    else void app.cmd("bulk_review_pillar", { pillarId: row.pillarId });
  }
</script>

<!-- Fixed height must equal PILLAR_HEADER_HEIGHT (132) in diffRenderModel.ts. -->
<div
  class="diff-viewport-row h-[132px] px-4 py-3 border-t-2 border-accent/40 border-b border-hairline bg-ink-850 flex flex-col gap-1.5 shrink-0 overflow-hidden"
  data-row-identity={row.identity}
>
  <div class="flex items-center gap-2">
    {#if row.foundation}
      <span class="text-[11px] text-accent" title="Foundation">◆</span>
    {/if}
    <h3 class="text-[14px] font-semibold text-fg truncate">{row.title}</h3>
    {#if allReviewed}
      <span class="text-[10px] px-1.5 py-[1px] rounded-full text-add-fg shrink-0" style="background: var(--color-add-bg);">Reviewed</span>
    {:else}
      <span class="mono text-[11px] text-muted shrink-0">{row.reviewedCount}/{row.totalCount} reviewed</span>
    {/if}
    <div class="flex-1"></div>
    <button
      class="text-[11px] px-2 py-[3px] rounded border border-hairline text-fg-2 hover:bg-card shrink-0"
      onclick={reviewAll}
    >{allReviewed ? "Unreview all" : "Review all"}</button>
  </div>
  {#if row.descriptionMarkdown}
    <p class="text-[12px] text-fg-2 leading-snug overflow-hidden" style="display:-webkit-box;-webkit-line-clamp:4;-webkit-box-orient:vertical;">{row.descriptionMarkdown}</p>
  {/if}
</div>
