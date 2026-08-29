<script lang="ts">
  import { SPLIT_ANNOTATION_TRAIL_PAD_PX } from "$lib/splitDiffLayout";
  import type { Snippet } from "svelte";

  interface Props {
    identity: string;
    split?: boolean;
    pane: "old" | "new";
    children: Snippet;
  }
  const { identity, split = false, pane, children }: Props = $props();
</script>

<div
  data-row-identity={identity}
  class={["annotation-inline-row", split && ["annotation-split-row", "split-diff-grid"]]}
  data-split-pane={split ? pane : undefined}
>
  <div
    class={["min-w-0", split && "annotation-split-slot"]}
    style={split ? `padding-right:${SPLIT_ANNOTATION_TRAIL_PAD_PX}px` : undefined}
  >
    {@render children()}
  </div>
</div>
