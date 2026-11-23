<script lang="ts">
  import { pageView } from "$lib/stores/pageView.svelte";
  import TopBar from "$lib/components/shell/TopBar.svelte";
  import type { AppSnapshot } from "$lib/pageTypes";

  interface Props {
    snapshot: AppSnapshot;
    /** Frame width, to show the narrow-width hiding. */
    width?: number;
  }
  const { snapshot, width }: Props = $props();

  $effect(() => {
    pageView.current = snapshot;
  });
</script>

<div class="bg-bg" style:width={width ? `${width}px` : undefined}>
  <TopBar />
</div>
