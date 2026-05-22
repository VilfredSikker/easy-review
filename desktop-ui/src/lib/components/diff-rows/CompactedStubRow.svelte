<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "compacted-stub" }>;
  }
  const { row }: Props = $props();

  function expand() {
    app.cmd("select_file", { idx: row.fileIndex });
    app.cmd("toggle_compacted");
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="px-4 py-3 text-muted text-sm italic cursor-pointer hover:bg-hover"
  style="height:{row.height}px;display:flex;align-items:center"
  data-row-identity={row.identity}
  onclick={expand}
>
  File compacted — click or press Enter to expand
</div>
