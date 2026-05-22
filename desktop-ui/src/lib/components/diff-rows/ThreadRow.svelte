<script lang="ts">
  import InlineThread from "$lib/components/InlineThread.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { ThreadSnapshot } from "$lib/types";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "inline-thread" | "fallback-thread" }>;
    thread: ThreadSnapshot;
  }
  const { row, thread }: Props = $props();
</script>

<div data-row-identity={row.identity}>
  <InlineThread {thread} hunk_idx={row.hunkIdx} />
</div>
