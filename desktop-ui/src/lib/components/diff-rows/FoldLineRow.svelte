<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "content-fold" }>;
  }
  const { row }: Props = $props();
</script>

<div
  class="grid grid-cols-[40px_minmax(0,1fr)] diff-row text-muted"
  style="height:{row.height}px"
  data-row-identity={row.identity}
>
  <div class="leading-6 text-right pr-2 gutter"></div>
  <div class="leading-6 pr-3 text-[12px] italic">{row.label}</div>
</div>
