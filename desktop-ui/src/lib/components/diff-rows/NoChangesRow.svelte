<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import { renamedFromLabel, similarityLabel } from "$lib/renameDiff";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "no-changes" }>;
  }
  const { row }: Props = $props();
  const similarity = $derived(similarityLabel(row.similarity));
</script>

<div
  class="px-4 py-3 text-fg-3 text-xs gap-2.5"
  style="height:{row.height}px;display:flex;align-items:center"
  data-row-identity={row.identity}
>
  {#if row.renamed}
    <span>Renamed · content unchanged</span>
    {#if row.oldPath}
      <span class="mono text-muted truncate" title={row.oldPath}>from {renamedFromLabel(row.oldPath, row.filePath)}</span>
    {/if}
    {#if similarity}<span class="text-muted">{similarity}</span>{/if}
  {:else}
    No changes
  {/if}
</div>
