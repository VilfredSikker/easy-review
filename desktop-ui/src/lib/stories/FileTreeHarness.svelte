<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
  }
  const { snapshot }: Props = $props();

  $effect(() => {
    app.snapshot = snapshot;
  });
</script>

<div class="h-[640px] w-64 border border-hairline bg-surface overflow-hidden">
  <FileTree />
</div>
