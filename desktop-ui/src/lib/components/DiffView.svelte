<script lang="ts">
  import type { DiffViewMode } from "$lib/stores/app.svelte";
  import FlatDiffView from "./FlatDiffView.svelte";

  interface Props {
    viewModeOverride?: DiffViewMode | null;
  }
  const { viewModeOverride = null }: Props = $props();
</script>

<FlatDiffView {viewModeOverride} />
