<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import type { AppSnapshot } from "$lib/types";
  import type { DiffViewMode } from "$lib/stores/app.svelte";

  interface Props {
    snapshot: AppSnapshot;
    viewModeOverride?: DiffViewMode | null;
  }
  const { snapshot, viewModeOverride = null }: Props = $props();

  $effect(() => { app.snapshot = snapshot; });
</script>

<div class="h-screen flex flex-col bg-[#0a0a0a] text-[#e8e8e8]">
  <DiffView {viewModeOverride} />
</div>
