<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { resolveActivePrUrl } from "$lib/prUrl";
  import { openExternalUrl } from "$lib/openExternalUrl";
  import type { DiffSourceSnapshot } from "$lib/types";

  const snapshot = $derived(app.snapshot);
  const tabs = $derived(snapshot?.tabs ?? []);
  const prUrl = $derived(resolveActivePrUrl(snapshot));
  const active = $derived(snapshot?.active_tab ?? 0);
  const activeTab = $derived(tabs.find((t) => t.is_active) ?? tabs[active]);
  const layout = $derived(browser.layout);

  // Derive additions and deletions by summing across all files.
  const additions = $derived((snapshot?.files ?? []).reduce((s, f) => s + f.additions, 0));
  const deletions = $derived((snapshot?.files ?? []).reduce((s, f) => s + f.deletions, 0));

  // Resolve the PR number for the badge.
  const prNumber = $derived(
    snapshot?.pr?.number ??
    snapshot?.github?.number ??
    activeTab?.pr_number ??
    null
  );

  // Diff source control.
  const diffSource = $derived(snapshot?.diff_source ?? null);

  let switchingSource = $state<"pr" | "origin" | "local" | null>(null);

  async function switchDiffSource(s: "pr" | "origin" | "local") {
    if (switchingSource) return;
    switchingSource = s;
    try {
      await app.cmd("set_diff_source", { source: s });
    } finally {
      switchingSource = null;
    }
  }

  const diffSourceLabels: Record<string, string> = {
    pr: "PR diff",
    origin: "Origin branch",
    local: "Local branch",
  };

  // Short labels for the segmented control (keep it terse).
  const segLabels: Record<string, string> = {
    pr: "PR diff",
    origin: "Origin",
    local: "Local branch",
  };

  async function copyBranchName() {
    const name = snapshot?.branch ?? "";
    if (!name) return;
    await copyToClipboard(name);
    app.showToast("success", "Branch name copied");
  }

  async function handlePrClick(e: MouseEvent) {
    if (!prUrl) return;
    if (e.metaKey || e.ctrlKey) {
      await openExternalUrl(prUrl);
    } else {
      await copyToClipboard(prUrl);
      app.showToast("success", "Copied PR link");
    }
  }

  async function revealWorktree() {
    const path = activeTab?.repo_root;
    if (!path) return;
    try {
      await invoke("reveal_path", { path });
      app.showToast("success", "Revealed in Finder");
    } catch (e) {
      app.showToast("error", `Reveal failed: ${e}`);
    }
  }
</script>

<div
  class="flex items-center h-9 border-b border-hairline bg-ink-870 shrink-0 px-3 gap-2"
  data-testid="branch-context-bar"
>
  <!-- Branch identity: glyph (orange) + full branch name + base chip + +/- summary -->
  <div class="flex items-center gap-1.5 min-w-0 shrink-0">
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-accent">
      <circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="12" r="2"/>
      <path d="M6 8v8M8 18h2a4 4 0 0 0 4-4v-2"/>
    </svg>
    <span class="text-fg text-sm font-medium whitespace-nowrap">{snapshot?.branch ?? "—"}</span>
    {#if snapshot?.base}
      <span class="text-muted text-[11px]">·</span>
      <span class="text-muted text-[11px]">base</span>
      <span class="px-1.5 py-0.5 rounded bg-ink-700 border border-hairline text-fg-3 text-[10px] font-mono whitespace-nowrap">{snapshot.base}</span>
    {/if}
    {#if additions > 0 || deletions > 0}
      <span class="font-mono text-[10px] text-add-fg">+{additions}</span>
      <span class="font-mono text-[10px] text-del-fg">−{deletions}</span>
    {/if}
  </div>

  <!-- Quick-action icon row -->
  <div class="flex items-center gap-0.5 ml-1">
    <!-- Copy branch name -->
    <button
      class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-muted hover:text-fg-2 transition-colors shrink-0"
      title="Copy branch name"
      onclick={copyBranchName}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="9" y="9" width="13" height="13" rx="2"/>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
      </svg>
    </button>

    <!-- Reveal worktree in Finder (stubbed — no backend command) -->
    <button
      class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-muted hover:text-fg-2 transition-colors shrink-0"
      title="Reveal worktree in Finder (not yet available)"
      onclick={revealWorktree}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
      </svg>
    </button>

    <!-- Open PR (with inline #NNNN badge) -->
    {#if prUrl}
      <button
        class="flex items-center gap-1 h-7 px-1.5 rounded hover:bg-ink-700 text-muted hover:text-fg-2 transition-colors shrink-0"
        title="Click to copy · ⌘-click to open"
        onclick={handlePrClick}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/>
          <path d="M13 6h3a2 2 0 0 1 2 2v7"/>
          <line x1="6" y1="9" x2="6" y2="21"/>
        </svg>
        {#if prNumber}
          <span class="text-[10px] text-fg-3 font-mono">#{prNumber}</span>
        {/if}
      </button>
    {/if}

    <!-- Terminal toggle (active state) -->
    <button
      class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors shrink-0 {terminal.open ? 'text-periwinkle bg-ink-700' : 'text-muted hover:text-fg-2'}"
      title={terminal.open ? "Hide terminal" : "Show terminal"}
      aria-pressed={terminal.open}
      onclick={() => terminal.toggle()}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="4 17 10 11 4 5"/>
        <line x1="12" y1="19" x2="20" y2="19"/>
      </svg>
    </button>

    <!-- Open split view (browser glyph, active state) -->
    <button
      type="button"
      class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors shrink-0 {layout === 'split' ? 'text-periwinkle bg-ink-700' : 'text-muted hover:text-fg-2'}"
      title={layout === "split" ? "Close split view" : "Open split view (⌘B)"}
      aria-pressed={layout === "split"}
      onclick={() => void browser.setLayout(layout === "split" ? "hidden" : "split")}
    >
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="3" y="3" width="18" height="18" rx="2"/>
        <path d="M12 3v18"/>
      </svg>
    </button>

  </div>

  <div class="flex-1 min-w-0"></div>

  <!-- PR diff | Local branch segmented control (right side) -->
  {#if diffSource && diffSource.available.length > 1}
    <div
      role="tablist"
      class="flex items-center bg-ink-800 border border-hairline rounded-md p-0.5 shrink-0"
    >
      {#each diffSource.available as s (s)}
        {@const isActive = s === diffSource.active}
        {@const isLoading = switchingSource === s}
        <button
          role="tab"
          aria-selected={isActive}
          disabled={isActive || !!switchingSource}
          onclick={() => switchDiffSource(s)}
          class="flex items-center gap-1 h-[22px] px-2.5 rounded text-[11px] font-medium transition-colors
            {isActive
              ? 'bg-ink-650 text-fg cursor-default'
              : 'text-muted hover:text-fg-2 disabled:opacity-40 disabled:cursor-not-allowed'}"
        >
          {#if isLoading}
            <span class="inline-block w-2.5 h-2.5 border border-current border-t-transparent rounded-full animate-spin"></span>
          {/if}
          {segLabels[s] ?? s}
        </button>
      {/each}
    </div>
  {/if}
</div>
