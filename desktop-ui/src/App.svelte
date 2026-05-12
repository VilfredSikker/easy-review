<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { initKeyboard } from "$lib/stores/keyboard";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";

  const panels = $derived(app.snapshot?.panels);

  onMount(() => {
    app.load().then(() => app.startPolling());
    const cleanupKeyboard = initKeyboard();
    return () => {
      cleanupKeyboard();
      app.stopPolling();
    };
  });
</script>

<div class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden">
  <header
    class="h-11 border-b border-ink-500/40 bg-ink-850 flex items-center gap-3 shrink-0 pr-4"
    style="padding-left: env(titlebar-area-x, 80px)"
  >
    <span class="text-sm font-medium text-ink-100 flex-1">
      {app.snapshot?.branch ?? "Easy Review"}
    </span>
    <span class="text-xs text-ink-300 font-mono">
      {app.snapshot?.base ? `← ${app.snapshot.base}` : ""}
    </span>
    <div class="flex items-center gap-1">
      <button
        class="p-1.5 rounded text-xs font-mono leading-none {panels?.left ? 'text-accent' : 'text-ink-300'} hover:bg-ink-700 transition-colors"
        onclick={() => app.togglePanel("left")}
        title="Toggle left sidebar [["
      >[</button>
      <button
        class="p-1.5 rounded text-xs font-mono leading-none {panels?.tree ? 'text-accent' : 'text-ink-300'} hover:bg-ink-700 transition-colors"
        onclick={() => app.togglePanel("tree")}
        title="Toggle file tree [\]"
      >\</button>
      <button
        class="p-1.5 rounded text-xs font-mono leading-none {panels?.right ? 'text-accent' : 'text-ink-300'} hover:bg-ink-700 transition-colors"
        onclick={() => app.togglePanel("right")}
        title="Toggle right panel []"
      >]</button>
    </div>
  </header>

  <div class="flex-1 flex min-h-0">
    {#if panels?.left}
      <LeftSidebar />
    {/if}

    <main class="flex-1 flex min-w-0">
      {#if panels?.tree}
        <FileTree />
      {/if}
      <DiffView />
    </main>

    {#if panels?.right}
      <RightPanel ai={app.snapshot?.ai ?? null} pr={app.snapshot?.pr ?? null} />
    {/if}
  </div>

  <footer class="h-7 shrink-0 bg-ink-850 border-t border-ink-500/40 flex items-center gap-4 px-4">
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">j/k</span> files</span>
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">n/N</span> hunks</span>
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">r</span> reviewed</span>
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">Enter</span> expand</span>
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">R</span> refresh</span>
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">[\/]</span> panels</span>
    <span class="text-xs text-ink-300"><span class="font-mono text-ink-200">Ctrl+Q</span> quit</span>
  </footer>
</div>
