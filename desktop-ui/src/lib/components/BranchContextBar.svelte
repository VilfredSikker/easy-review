<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { resolveActivePrUrl } from "$lib/prUrl";
  import { resolveTabRoot } from "$lib/resolveTabRoot";
  import { openExternalUrl } from "$lib/openExternalUrl";

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
  const currentWorktree = $derived(snapshot?.worktrees?.find((w) => w.is_current) ?? null);
  // Mirror resolveActivePrUrl's resolution exactly, so the toggle appears
  // whenever the header's #NNNN PR button does (incl. the worktree's pr_number,
  // which is the source for branches whose PR isn't in the bulk pr_cache).
  const prNumber = $derived(
    snapshot?.detected_pr_number ??
    snapshot?.github?.number ??
    snapshot?.pr?.number ??
    activeTab?.pr_number ??
    currentWorktree?.pr_number ??
    null
  );

  const mode = $derived(snapshot?.mode ?? "branch");
  /** PR Diff is "active" in PR mode, and also in Guide mode when the guide is
   *  attached to the PR diff — entering the Guide must not flip the toggle to
   *  Local Branch. */
  const prActive = $derived(
    mode === "pr" || (mode === "tour" && snapshot?.tour?.scope === "pr"),
  );
  /** Show the [Local Branch | PR Diff] toggle when the branch has a PR, the
   *  tab is local (remote-only tabs are implicitly PR Diff), AND the head
   *  branch is checked out. Without a checkout there's no working-tree "Local
   *  Branch" view distinct from PR Diff (both would be `gh pr diff`), so the
   *  toggle is hidden and the tab is PR Diff only. */
  const showSourceToggle = $derived(
    prNumber != null
      && activeTab?.kind !== "remote_pr"
      && snapshot?.local_branch_checked_out === true,
  );

  /** Set when the open diff is behind origin (PR head or base advanced). */
  const diffStale = $derived(snapshot?.diff_stale ?? null);

  let syncing = $state(false);
  async function syncStale() {
    if (syncing) return;
    syncing = true;
    try {
      await app.cmd("force_refresh_diff");
    } finally {
      syncing = false;
    }
  }

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

  /** Resolved local worktree path for the active tab. Remote-only PR tabs
   *  have no local checkout, so the resolved path is empty and the button hides. */
  const worktreePath = $derived(resolveTabRoot(snapshot, activeTab) || null);

  async function handleWorktreeClick(e: MouseEvent) {
    const path = worktreePath;
    if (!path) return;
    if (e.metaKey || e.ctrlKey) {
      try {
        await invoke("reveal_path", { path });
        app.showToast("success", "Revealed in Finder");
      } catch (err) {
        app.showToast("error", `Reveal failed: ${err}`);
      }
    } else {
      await copyToClipboard(path);
      app.showToast("success", "Worktree path copied");
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

    <!-- Worktree path: click copies · ⌘-click reveals in Finder -->
    {#if worktreePath}
      <button
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-muted hover:text-fg-2 transition-colors shrink-0"
        title="Click to copy worktree path · ⌘-click to reveal in Finder"
        onclick={handleWorktreeClick}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M5 5a2 2 0 0 1 2-2h3l2 2h4a2 2 0 0 1 2 2"/>
          <path d="M2 10a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z"/>
        </svg>
      </button>
    {/if}

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

  <!-- Stale-diff pill + Sync (right side, before the source toggle) -->
  {#if diffStale}
    <div
      class="flex items-center gap-1 h-[22px] pl-2 pr-1 rounded-md bg-warning/15 border border-warning/40 shrink-0"
      title={diffStale.message}
    >
      <span class="text-[10px] font-medium uppercase tracking-wide text-warning">Stale</span>
      <button
        class="w-5 h-5 rounded flex items-center justify-center text-warning hover:bg-warning/20 transition-colors disabled:opacity-50 disabled:cursor-default"
        title={diffStale.message}
        disabled={syncing}
        onclick={syncStale}
      >
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class={syncing ? "animate-spin" : ""}
        >
          <path d="M3 12a9 9 0 0 1 15-6.7L21 8" />
          <path d="M21 3v5h-5" />
          <path d="M21 12a9 9 0 0 1-15 6.7L3 16" />
          <path d="M3 21v-5h5" />
        </svg>
      </button>
    </div>
  {/if}

  <!-- Local Branch | PR Diff segmented toggle (right side) -->
  {#if showSourceToggle}
    <div role="tablist" class="flex items-center bg-ink-800 border border-hairline rounded-md p-0.5 shrink-0">
      <button
        role="tab"
        aria-selected={!prActive}
        disabled={!prActive}
        onclick={() => void app.cmd("set_mode", { mode: "branch" })}
        class="h-[22px] px-2.5 rounded text-[11px] font-medium transition-colors {!prActive ? 'bg-ink-650 text-fg cursor-default' : 'text-muted hover:text-fg-2'}"
      >
        Local Branch
      </button>
      <button
        role="tab"
        aria-selected={prActive}
        disabled={prActive}
        onclick={() => void app.cmd("set_mode", { mode: "pr_diff", prNumber: prNumber })}
        class="flex items-center gap-1 h-[22px] px-2.5 rounded text-[11px] font-medium transition-colors {prActive ? 'bg-ink-650 text-fg cursor-default' : 'text-muted hover:text-fg-2'}"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7"/><line x1="6" y1="9" x2="6" y2="21"/></svg>
        PR Diff
      </button>
    </div>
  {/if}
</div>
