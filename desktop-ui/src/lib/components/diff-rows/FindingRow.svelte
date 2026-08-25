<script lang="ts">
  import InlineFinding from "$lib/components/InlineFinding.svelte";
  import { findingReviewSide } from "$lib/diffAnnotations";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { FlatFinding, LineSnapshot, ThreadSnapshot } from "$lib/types";
  import AnnotationSplitRow from "./AnnotationSplitRow.svelte";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "inline-finding" | "fallback-finding" }>;
    finding: FlatFinding;
    thread: ThreadSnapshot | null;
    /** Constrain the card to the old/new pane (split diff). */
    split?: boolean;
    hunkLines?: LineSnapshot[];
  }
  const { row, finding, thread, split = false, hunkLines = [] }: Props = $props();
</script>

<AnnotationSplitRow identity={row.identity} {split} pane={findingReviewSide(finding, hunkLines)}>
  <InlineFinding {finding} {thread} />
</AnnotationSplitRow>
