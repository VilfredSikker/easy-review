<script lang="ts">
  import InlineThread from "$lib/components/InlineThread.svelte";
  import { threadReviewSide } from "$lib/diffAnnotations";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { ThreadSnapshot } from "$lib/types";
  import AnnotationSplitRow from "./AnnotationSplitRow.svelte";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "inline-thread" | "fallback-thread" }>;
    thread: ThreadSnapshot;
    /** Constrain the card to the old/new pane (split diff). */
    split?: boolean;
  }
  const { row, thread, split = false }: Props = $props();
</script>

<AnnotationSplitRow identity={row.identity} {split} pane={threadReviewSide(thread)}>
  <InlineThread {thread} hunk_idx={row.hunkIdx} />
</AnnotationSplitRow>
