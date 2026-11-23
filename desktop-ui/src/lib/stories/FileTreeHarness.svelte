<script lang="ts">
  import { pageView } from "$lib/stores/pageView.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import type { AppSnapshot } from "$lib/pageTypes";

  interface Props {
    snapshot: AppSnapshot;
  }
  const { snapshot }: Props = $props();

  $effect(() => {
    pageView.current = snapshot;
  });
</script>

<div class="h-[640px] w-64 border border-hairline bg-surface overflow-hidden">
  <FileTree />
</div>
