<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import TopBar from "$lib/components/shell/TopBar.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
    /** Frame width, to show the narrow-width hiding. */
    width?: number;
  }
  const { snapshot, width }: Props = $props();

  $effect(() => {
    app.snapshot = snapshot;
  });
</script>

<div class="bg-bg" style:width={width ? `${width}px` : undefined}>
  <TopBar />
</div>
