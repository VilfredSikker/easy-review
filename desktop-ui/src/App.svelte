<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";

  onMount(() => app.load());
</script>

<div class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden">
  <!-- Top bar -->
  <header class="h-11 border-b border-ink-500/40 bg-ink-850 flex items-center px-4 gap-3 shrink-0" style="padding-left: env(titlebar-area-x, 80px)">
    <span class="text-sm font-medium text-ink-100">{app.snapshot?.branch ?? "Easy Review"}</span>
    <span class="text-xs text-ink-300 font-mono">{app.snapshot?.base ? `← ${app.snapshot.base}` : ""}</span>
  </header>

  <!-- Main content -->
  <div class="flex-1 flex min-h-0">
    <!-- Left sidebar placeholder -->
    <aside class="w-11 border-r border-ink-500/40 bg-ink-850 shrink-0"></aside>

    <!-- File tree + diff -->
    <main class="flex-1 flex min-w-0">
      <FileTree />
      <DiffView />
    </main>

    <!-- Right panel placeholder -->
    <aside class="w-80 border-l border-ink-500/40 bg-ink-850 shrink-0 p-4">
      {#if app.snapshot?.ai}
        <div class="text-xs text-ink-300 mb-2 uppercase tracking-wider">AI Review</div>
        <div class="text-sm text-ink-200">
          <span class="text-risk-high">{app.snapshot.ai.high}H</span>
          <span class="mx-1 text-ink-400">·</span>
          <span class="text-risk-med">{app.snapshot.ai.med}M</span>
          <span class="mx-1 text-ink-400">·</span>
          <span class="text-risk-low">{app.snapshot.ai.low}L</span>
        </div>
      {/if}
    </aside>
  </div>
</div>
