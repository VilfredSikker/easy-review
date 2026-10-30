<script lang="ts">
  import type { BackgroundTaskSnapshot } from "$lib/types";
  import RunningPill from "./shell/RunningPill.svelte";

  /** Floating home for the running-AI pill while the shell has no top bar.
   *  Once `TopBar` is mounted it carries the pill, and this mount goes. */
  interface Props {
    /** Kept for the existing mount; the pill reads the snapshot itself. */
    tasks?: BackgroundTaskSnapshot[];
    avoidRightPanel?: boolean;
    rightPanelWidth?: number;
  }
  const { avoidRightPanel = false, rightPanelWidth = 340 }: Props = $props();

  const rightOffset = $derived(avoidRightPanel ? rightPanelWidth + 16 : 24);
</script>

<div class="fixed bottom-10 z-50 flex justify-end" style="right: {rightOffset}px" aria-label="Background tasks">
  <!-- The pill's tint is translucent; over the diff it needs a solid ground. -->
  <div class="rounded-full bg-card shadow">
    <RunningPill />
  </div>
</div>
