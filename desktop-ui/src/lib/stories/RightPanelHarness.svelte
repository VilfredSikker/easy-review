<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { rightPanelTab, type RightPanelTab } from "$lib/stores/rightPanelTab.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import CollapsedRightRail from "$lib/components/CollapsedRightRail.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
    tab?: RightPanelTab;
    collapsed?: boolean;
  }

  const { snapshot, tab = "findings", collapsed = false }: Props = $props();

  // The panel reads the global store, as it does in the app.
  $effect(() => {
    app.snapshot = snapshot;
  });
  $effect(() => {
    rightPanelTab.set(tab);
  });
</script>

<div class="h-screen flex justify-end bg-bg text-fg font-sans">
  {#if collapsed}
    <CollapsedRightRail ai={snapshot.ai} onExpand={(t) => rightPanelTab.set(t)} />
  {:else}
    <RightPanel ai={snapshot.ai} pr={snapshot.pr} />
  {/if}
</div>
