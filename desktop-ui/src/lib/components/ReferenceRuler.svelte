<script lang="ts">
  /**
   * Overview ruler for reference-highlight matches (issue #69) — a thin
   * absolutely-positioned strip on the right edge of the diff scroll
   * viewport, like the scrollbar markers in VSCode/Zed. One mark per matched
   * row (overlapping marks are pre-merged by `buildRulerMarks`); clicking a
   * mark jumps to that match. Rendered only while a highlight is active, so
   * it appears/clears with the highlight itself.
   */
  import type { RulerMark } from "$lib/referenceUsages";

  interface Props {
    marks: RulerMark[];
    onJump: (rowIdx: number) => void;
  }
  const { marks, onJump }: Props = $props();
</script>

<div class="ref-ruler" aria-label="Reference match positions">
  {#each marks as m (m.rowIdx)}
    <button
      type="button"
      class="ref-ruler-mark"
      style="top:{m.topPx}px"
      tabindex="-1"
      aria-label="Jump to reference match"
      onclick={() => onJump(m.rowIdx)}
    ></button>
  {/each}
</div>
