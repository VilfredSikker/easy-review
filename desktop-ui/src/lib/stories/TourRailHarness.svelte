<script lang="ts">
  import type { ComponentProps } from "svelte";
  import TourRail from "$lib/components/TourRail.svelte";

  const props: ComponentProps<typeof TourRail> = $props();
</script>

<!-- The app mounts the rail inside FlatDiffView's `.tour-split`: a flex row
     of definite height with the column filling the rest. Without that row the
     ruler's strip drops below the rail and collapses to its content height. -->
<div class="split">
  <TourRail {...props} />
  <div class="column"></div>
</div>

<style>
  .split {
    display: flex;
    height: 100vh;
    min-height: 0;
    background: var(--color-bg);
  }
  .column {
    flex: 1;
    min-width: 0;
  }
  @media (max-width: 880px) {
    .split {
      flex-direction: column;
    }
  }
</style>
