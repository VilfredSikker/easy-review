<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { copyToClipboard } from "$lib/clipboard";

  const tabs = $derived(app.snapshot?.tabs ?? []);
  const active = $derived(app.snapshot?.active_tab ?? 0);
  const activeTab = $derived(tabs.find((t) => t.is_active) ?? tabs[active]);
  const activeTabRoot = $derived(
    app.snapshot?.worktrees?.find((w) => w.branch === activeTab?.branch)?.path ??
      activeTab?.repo_root ??
      "",
  );
</script>

<div
  class="flex items-center h-9 border-b border-ink-650 bg-ink-870 shrink-0 pl-0 pr-3"
  data-testid="branch-context-bar"
>
  <div class="flex items-center gap-1 min-w-0">
    <div class="flex items-center gap-2 px-2.5 py-1 rounded-md bg-ink-700 border border-ink-500 text-sm cursor-default max-w-[14rem] min-w-0">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
      <span class="text-ink-100 text-sm truncate">{app.snapshot?.branch ?? "Review"}</span>
      {#if app.snapshot?.base}
        <span class="font-mono text-[10px] text-ink-300 shrink-0">{app.snapshot.base}</span>
      {/if}
    </div>
    <button
      class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-ink-300 hover:text-ink-100 transition-colors shrink-0"
      title="Copy branch name"
      onclick={() => copyToClipboard(app.snapshot?.branch ?? "")}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
    </button>
    {#if activeTabRoot && activeTab?.kind !== "remote_pr"}
      <button
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-ink-300 hover:text-ink-100 transition-colors shrink-0"
        title="Copy repo path"
        onclick={() => copyToClipboard(activeTabRoot)}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
      </button>
    {/if}
  </div>
</div>
