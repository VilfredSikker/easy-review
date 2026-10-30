<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import ViewBar from "$lib/components/shell/ViewBar.svelte";
  import StaleBanner from "$lib/components/shell/StaleBanner.svelte";
  import LocalBranchBanner from "$lib/components/shell/LocalBranchBanner.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
  }
  const { snapshot }: Props = $props();

  $effect(() => {
    app.snapshot = snapshot;
  });
</script>

<div class="min-h-[320px] bg-bg text-fg">
  <ViewBar />
  <StaleBanner />
  <LocalBranchBanner />
</div>
