<script lang="ts">
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "lazy-stub" }>;
  }
  const { row }: Props = $props();
</script>

<div
  class="px-4 py-3 text-muted text-sm"
  style="height:{row.height}px;display:flex;align-items:flex-start"
  data-row-identity={row.identity}
>
  Loading content…
</div>
