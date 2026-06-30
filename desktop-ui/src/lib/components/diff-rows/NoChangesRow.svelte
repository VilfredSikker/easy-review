<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "no-changes" }>;
  }
  const { row }: Props = $props();
</script>

<div
  class="px-4 py-3 text-muted text-sm"
  style="height:{row.height}px;display:flex;align-items:center"
  data-row-identity={row.identity}
>
  {row.renamed ? "File renamed without changes." : "No changes"}
</div>
