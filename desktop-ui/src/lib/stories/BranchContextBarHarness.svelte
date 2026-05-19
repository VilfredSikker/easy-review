<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import BranchContextBar from "$lib/components/BranchContextBar.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
  }
  const { snapshot }: Props = $props();

  $effect(() => {
    app.snapshot = snapshot;
  });
</script>

<div class="bg-ink-900">
  <BranchContextBar />
</div>
