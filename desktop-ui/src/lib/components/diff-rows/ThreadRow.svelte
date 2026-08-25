<script lang="ts">
  import InlineThread from "$lib/components/InlineThread.svelte";
  import { splitThreadGridColumn, threadReviewSide } from "$lib/diffAnnotations";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { ThreadSnapshot } from "$lib/types";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "inline-thread" | "fallback-thread" }>;
    thread: ThreadSnapshot;
    /** Constrain the card to the old/new pane (split diff). */
    split?: boolean;
  }
  const { row, thread, split = false }: Props = $props();
  const pane = $derived(threadReviewSide(thread));
</script>

<div
  data-row-identity={row.identity}
  class={["annotation-inline-row", split && "annotation-split-row"]}
  data-split-pane={split ? pane : undefined}
>
  <div
    class={["min-w-0", split && "annotation-split-slot"]}
    style={split
      ? `grid-column:${splitThreadGridColumn(thread)};padding-right:8px`
      : undefined}
  >
    <InlineThread {thread} hunk_idx={row.hunkIdx} />
  </div>
</div>
