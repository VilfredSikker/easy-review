<script lang="ts">
  import { untrack } from "svelte";
  import DiffComposer from "$lib/components/DiffComposer.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";

  /**
   * Storybook harness for the three composer placements. The diff view decides
   * which one is live (anchor row in the window → `flow`, anchor known but
   * scrolled away → `absolute`, no rendered anchor row → `sticky`); this pins
   * each one in isolation.
   */
  interface Props {
    placement: { kind: "flow" } | { kind: "absolute"; topPx: number } | { kind: "sticky" };
    splitPane?: "old" | "new" | null;
    kind?: "comment" | "question" | "note";
    text?: string;
    range?: [number, number];
  }
  const {
    placement,
    splitPane = null,
    kind = "comment",
    text = "",
    range = [36, 39],
  }: Props = $props();

  $effect(() => {
    // Same shape the diff view leaves behind when a drag-select finishes.
    untrack(() => {
      diffSel.start = range[0];
      diffSel.end = range[1];
      diffSel.file = "src/example/MediaCombobox.svelte";
      diffSel.side = splitPane ?? "new";
      diffSel.kind = kind;
      diffSel.text = text;
      diffSel.dragging = false;
      diffSel.focusPending = false;
    });
  });
</script>

<!-- Mirrors the diff view's scroll container: `sticky` docks to this box, and
     `absolute` resolves against it. Filler stands in for the code rows. -->
<div
  class="relative h-[420px] overflow-y-auto bg-bg text-fg-2 mono text-[13px]"
  style="border:1px solid var(--color-hairline)"
>
  {#each Array(24) as _, i (i)}
    <div class="h-6 px-4 leading-6 {i === 8 ? 'bg-diff-add-bg' : ''}">
      {i === 8 ? "const selected = true;" : `  // context line ${i}`}
    </div>
  {/each}
  <div class="h-64"></div>

  {#if placement.kind === "flow"}
    <!-- In flow, below the anchor row. flow-root keeps the card's margins in. -->
    <div class="composer-flow-row" style="display:flow-root">
      <DiffComposer {placement} {splitPane} />
    </div>
  {:else}
    <DiffComposer {placement} {splitPane} />
  {/if}
</div>
