<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import TreesFileTreeSpike from "$lib/spikes/TreesFileTreeSpike.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import type { AppSnapshot } from "$lib/types";

  interface Props {
    snapshot: AppSnapshot;
    /** Side-by-side with the production FileTree for comparison. */
    compareEr?: boolean;
  }

  const { snapshot, compareEr = false }: Props = $props();

  let files = $state([...snapshot.files]);
  let selectedPath = $state<string | null>(files[0]?.path ?? null);

  $effect(() => {
    if (!compareEr) return;
    const idx = selectedPath
      ? files.findIndex((f) => f.path === selectedPath)
      : snapshot.selected_file;
    app.snapshot = {
      ...snapshot,
      files,
      selected_file: idx >= 0 ? idx : snapshot.selected_file,
    };
  });

  function bumpStats() {
    files = files.map((f) => ({
      ...f,
      additions: f.additions + 3,
      deletions: f.deletions + 1,
      cache_key: `${f.cache_key}-bump`,
    }));
  }

  function onSelect(path: string) {
    selectedPath = path;
  }
</script>

<div class="h-screen flex flex-col bg-bg text-fg">
  <header class="shrink-0 flex items-center gap-3 px-4 py-2 border-b border-hairline text-sm">
    <span class="font-medium text-fg">@pierre/trees spike</span>
    <span class="text-muted">read-only · git status · icons · +/− decorations · search</span>
    <div class="ml-auto flex items-center gap-2">
      <button
        type="button"
        class="px-2 py-1 rounded text-xs bg-hover border border-hairline hover:bg-panel"
        onclick={bumpStats}
      >
        Simulate watch (+3/−1)
      </button>
      {#if selectedPath}
        <span class="mono text-[11px] text-muted truncate max-w-[280px]" title={selectedPath}>
          {selectedPath}
        </span>
      {/if}
    </div>
  </header>

  <div class="flex-1 min-h-0 flex {compareEr ? 'divide-x divide-hairline' : ''}">
    {#if compareEr}
      <section class="w-64 shrink-0 flex flex-col min-h-0">
        <p class="px-3 py-1 text-[10px] mono text-muted border-b border-hairline">er FileTree</p>
        <div class="flex-1 min-h-0">
          <FileTree />
        </div>
      </section>
    {/if}
    <section class="flex-1 min-h-0 flex flex-col">
      {#if compareEr}
        <p class="px-3 py-1 text-[10px] mono text-muted border-b border-hairline">trees.software</p>
      {/if}
      <div class="flex-1 min-h-0">
        <TreesFileTreeSpike {files} {selectedPath} onSelect={onSelect} />
      </div>
    </section>
  </div>
</div>
