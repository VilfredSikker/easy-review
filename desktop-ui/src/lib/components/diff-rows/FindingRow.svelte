<script lang="ts" module>
  import type { LineSnapshot } from "$lib/types";
  const EMPTY_HUNK_LINES: LineSnapshot[] = [];
</script>

<script lang="ts">
  import InlineFinding from "$lib/components/InlineFinding.svelte";
  import { findingReviewSide } from "$lib/diffAnnotations";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { FlatFinding, ThreadSnapshot } from "$lib/types";
  import AnnotationSplitRow from "./AnnotationSplitRow.svelte";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "inline-finding" | "fallback-finding" }>;
    finding: FlatFinding;
    thread: ThreadSnapshot | null;
    /** Constrain the card to the old/new pane (split diff). */
    split?: boolean;
    hunkLines?: LineSnapshot[];
  }
  const { row, finding, thread, split = false, hunkLines = EMPTY_HUNK_LINES }: Props = $props();
</script>

<AnnotationSplitRow identity={row.identity} {split} pane={findingReviewSide(finding, hunkLines)}>
  <InlineFinding {finding} {thread} />
</AnnotationSplitRow>
