<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { initKeyboard } from "$lib/stores/keyboard";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import BottomHints from "$lib/components/BottomHints.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import ExportModal from "$lib/components/ExportModal.svelte";
  import TabStrip from "$lib/components/TabStrip.svelte";
  import Terminal from "$lib/components/Terminal.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import BrowserView from "$lib/components/BrowserView.svelte";
  import { browser } from "$lib/stores/browser.svelte";

  const panels = $derived(app.snapshot?.panels);
  const worktrees = $derived(app.snapshot?.worktrees ?? []);
  const multipleWorktrees = $derived(worktrees.length > 1);
  /** No snapshot yet (initial load) OR snapshot has no branch (no repo open). */
  const isEmpty = $derived(
    app.showEmptyState ||
      app.snapshot === null ||
      (app.snapshot.branch === "" && app.snapshot.files.length === 0),
  );

  let showWorktrees = $state(false);
  let drawerHeight = $state(280);

  // --- Terminal drawer resize ---------------------------------------------
  // Drag the 4px handle along the drawer's top edge to resize. Height is
  // clamped to a sane range and persisted to localStorage on drag-end so it
  // survives app restarts.
  const DRAWER_MIN = 100;
  const DRAWER_STORAGE_KEY = "terminalDrawerHeight";

  function clampDrawerHeight(h: number): number {
    const max = Math.max(DRAWER_MIN, window.innerHeight * 0.85);
    return Math.min(max, Math.max(DRAWER_MIN, h));
  }

  let dragging = $state(false);

  function onResizeStart(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    const startY = e.clientY;
    const startH = drawerHeight;

    const onMove = (ev: MouseEvent) => {
      // Dragging up grows the drawer; dragging down shrinks it.
      drawerHeight = clampDrawerHeight(startH + (startY - ev.clientY));
    };
    const onUp = () => {
      dragging = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      try {
        localStorage.setItem(DRAWER_STORAGE_KEY, String(drawerHeight));
      } catch {
        /* ignore */
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  const activeTabIdx = $derived(app.snapshot?.active_tab ?? 0);
  const activeTabRoot = $derived(
    app.snapshot?.tabs?.[activeTabIdx]?.repo_root ?? "",
  );

  onMount(() => {
    // Restore persisted drawer height before the drawer can render.
    try {
      const raw = localStorage.getItem(DRAWER_STORAGE_KEY);
      const parsed = raw ? Number(raw) : NaN;
      if (Number.isFinite(parsed)) drawerHeight = clampDrawerHeight(parsed);
    } catch {
      /* ignore */
    }
    app.load().then(() => app.startPolling());
    const cleanupKeyboard = initKeyboard();
    return () => {
      cleanupKeyboard();
      app.stopPolling();
    };
  });
</script>

{#if isEmpty && !app.loading}
  <EmptyState />
{:else}

<div class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden">
  <TabStrip />

  <header
    class="h-11 border-b border-ink-650 bg-ink-870 flex items-center gap-1 shrink-0 pr-3 pl-3"
  >
    <!-- left panel + nav buttons -->
    <div class="flex items-center gap-0.5 mr-3 text-ink-300">
      <button
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors {panels?.left ? 'text-accent bg-ink-700' : ''}"
        onclick={() => app.togglePanel("left")}
        title="Toggle left panel [["
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/></svg>
      </button>
      <button class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 opacity-40 transition-colors" title="Back">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="15 18 9 12 15 6"/></svg>
      </button>
      <button class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 opacity-40 transition-colors" title="Forward">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
      </button>
    </div>

    <!-- tab strip -->
    <div class="relative flex items-center gap-1 min-w-0">
      <div class="flex items-center gap-2 px-3 py-1 rounded-md bg-ink-700 border border-ink-500 text-sm cursor-default">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
        <span class="text-ink-100 text-sm truncate">{app.snapshot?.branch ?? "Review"}</span>
        {#if app.snapshot?.base}
          <span class="font-mono text-[10px] text-ink-300">{app.snapshot.base}</span>
        {/if}
        {#if multipleWorktrees}
          <button
            class="shrink-0 text-ink-300 hover:text-ink-100 transition-colors"
            onclick={() => (showWorktrees = !showWorktrees)}
            title="Worktrees"
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <path d="M6 9l6 6 6-6"/>
            </svg>
          </button>
        {/if}
      </div>
      <button class="w-7 h-7 rounded hover:bg-ink-700 flex items-center justify-center text-ink-300 hover:text-ink-100 transition-colors" title="New tab">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
      </button>
      <!-- worktree dropdown -->
      {#if showWorktrees && multipleWorktrees}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="fixed inset-0 z-40" onclick={() => (showWorktrees = false)}></div>
        <div class="absolute left-0 top-full mt-1 z-50 bg-ink-800 border border-ink-500 rounded shadow-xl min-w-[220px]">
          {#each worktrees as wt}
            <div class="px-3 py-2 flex items-center gap-2 {wt.is_current ? 'bg-ink-700' : 'hover:bg-ink-750'} cursor-default">
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
    </div>

    <div class="flex-1"></div>

    <!-- right side -->
    <div class="flex items-center gap-0.5 text-ink-300">
      {#if app.snapshot?.watch_active}
        <span class="w-1.5 h-1.5 rounded-full bg-add-fg/60 shrink-0 mr-1.5" title="Watch active"></span>
      {/if}
      <button
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors {panels?.tree ? 'text-accent bg-ink-700' : ''}"
        onclick={() => app.togglePanel("tree")}
        title="Toggle file tree [\]"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z"/></svg>
      </button>
      <button class="text-xs text-ink-200 hover:bg-ink-700 px-3 py-1 rounded-md font-mono transition-colors">⌘K</button>
      <button
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors {panels?.right ? 'text-accent bg-ink-700' : ''}"
        onclick={() => app.togglePanel("right")}
        title="Toggle right panel []]"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M15 3v18"/></svg>
      </button>
    </div>
  </header>

  <div class="flex-1 flex min-h-0">
    {#if !browser.open}
      <LeftSidebar collapsed={!panels?.left} />
    {/if}

    <main class="flex-1 flex min-w-0">
      {#if !browser.open}
        <FileTree collapsed={!panels?.tree} />
        <DiffView />
      {:else}
        <BrowserView />
      {/if}
    </main>

    {#if panels?.right && !browser.open}
      <RightPanel ai={app.snapshot?.ai ?? null} pr={app.snapshot?.pr ?? null} />
    {/if}
  </div>

  {#if terminal.open && !browser.open}
    <div
      class="relative border-t border-hairline bg-ink-900 shrink-0"
      style="height: {drawerHeight}px"
    >
      <!--
        4px drag handle along the top edge. Sits above the border, captures
        mousedown to start resizing. While dragging we set `cursor: ns-resize`
        on body via the `dragging` class so the cursor doesn't flicker when
        the pointer briefly leaves the handle.
      -->
      <div
        class="absolute -top-[2px] left-0 right-0 h-1 cursor-ns-resize z-10 hover:bg-accent/40 {dragging ? 'bg-accent/60' : ''}"
        onmousedown={onResizeStart}
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize terminal drawer"
      ></div>
      <Terminal
        sessionId={`tab-${activeTabIdx}`}
        cwd={activeTabRoot}
        visible={terminal.open}
      />
    </div>
  {/if}

  {#if !browser.open}
    <BottomHints />
  {/if}

  <Toast message={app.snapshot?.notification ?? null} />
  {#if app.error}
    <div class="fixed bottom-12 left-1/2 -translate-x-1/2 bg-[#3a1a1a] border border-[#f4a3a3] text-[#f4a3a3] text-xs font-mono px-4 py-2 rounded shadow-lg z-50 max-w-[80vw] truncate">
      ⚠ {app.error}
    </div>
  {/if}
  <CommandPalette />
  <ExportModal />
</div>
{/if}
