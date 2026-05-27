<script lang="ts">
  import FileHeaderContent from "./FileHeaderContent.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "file-header" }>;
  }
  const { row }: Props = $props();
</script>

<div
  class="h-10 px-3 border-t border-ink-650 border-b border-hairline bg-ink-800 flex items-center gap-2 shrink-0"
  data-row-identity={row.identity}
>
  <FileHeaderContent {row} />
</div>
