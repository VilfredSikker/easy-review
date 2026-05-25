<script lang="ts">
  import InlineFinding from "$lib/components/InlineFinding.svelte";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { FlatFinding, ThreadSnapshot } from "$lib/types";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "inline-finding" | "fallback-finding" }>;
    finding: FlatFinding;
    thread: ThreadSnapshot | null;
  }
  const { row, finding, thread }: Props = $props();
</script>

<div data-row-identity={row.identity} class="annotation-inline-row">
  <InlineFinding {finding} {thread} />
</div>
