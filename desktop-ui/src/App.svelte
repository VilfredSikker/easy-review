<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { initKeyboard } from "$lib/stores/keyboard";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import Toast from "$lib/components/Toast.svelte";

  const panels = $derived(app.snapshot?.panels);
  const worktrees = $derived(app.snapshot?.worktrees ?? []);
  const multipleWorktrees = $derived(worktrees.length > 1);

  let showWorktrees = $state(false);

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
    <!-- Branch / worktree picker -->
    <div class="relative flex items-center gap-1.5 min-w-0">
      <span class="text-sm font-medium text-ink-100 truncate">
        {app.snapshot?.branch ?? "Easy Review"}
      </span>
      {#if multipleWorktrees}
        <button
          class="shrink-0 text-ink-400 hover:text-ink-200 transition-colors"
          onclick={() => (showWorktrees = !showWorktrees)}
          title="Worktrees"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M6 9l6 6 6-6"/>
          </svg>
        </button>
        {#if showWorktrees}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="fixed inset-0 z-40"
            onclick={() => (showWorktrees = false)}
          ></div>
          <div class="absolute left-0 top-full mt-1 z-50 bg-ink-800 border border-ink-500/40 rounded shadow-xl min-w-[220px]">
            {#each worktrees as wt}
              <div
                class="px-3 py-2 flex items-center gap-2 {wt.is_current ? 'bg-ink-700' : 'hover:bg-ink-750'} cursor-default"
              >
                {#if wt.is_current}
                  <span class="text-accent text-xs shrink-0">●</span>
                {:else}
                  <span class="w-3 shrink-0"></span>
                {/if}
                <div class="flex flex-col min-w-0">
                  <span class="text-xs text-ink-100 font-mono truncate">{wt.branch}</span>
                  <span class="text-[10px] text-ink-400 truncate">{wt.path.split("/").slice(-2).join("/")}</span>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>

    <span class="text-xs text-ink-300 font-mono shrink-0">
      {app.snapshot?.base ? `← ${app.snapshot.base}` : ""}
    </span>

    <div class="flex-1"></div>

    <!-- Watch indicator -->
    {#if app.snapshot?.watch_active}
      <span class="w-1.5 h-1.5 rounded-full bg-add-fg/60 shrink-0" title="Watch active"></span>
    {/if}

    <!-- Panel toggles -->
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

  <Toast message={app.snapshot?.notification ?? null} />

  <footer class="h-7 shrink-0 bg-ink-850 border-t border-ink-500/40 flex items-center gap-4 px-4 overflow-x-auto">
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">j/k</span> files</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">n/N</span> hunks</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">r</span> reviewed</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">e</span> editor</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">R</span> refresh</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">g/p</span> gh sync</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">[\/]</span> panels</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">1-4</span> mode</span>
    <span class="text-xs text-ink-300 shrink-0"><span class="font-mono text-ink-200">Ctrl+Q</span> quit</span>
  </footer>
</div>
