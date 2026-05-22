<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "hunk-header" }>;
  }
  const { row }: Props = $props();
</script>

<div
  class="px-4 py-1 text-muted bg-surface border-b border-hairline text-[12px]"
  style="height:{row.height}px;display:flex;align-items:center"
  data-row-identity={row.identity}
>
  {row.header}
</div>
