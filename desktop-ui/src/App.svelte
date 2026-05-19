<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { initKeyboard } from "$lib/stores/keyboard";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import BackgroundTasks from "$lib/components/BackgroundTasks.svelte";
  import BottomHints from "$lib/components/BottomHints.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import AiActionPalette from "$lib/components/AiActionPalette.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import ExportModal from "$lib/components/ExportModal.svelte";
  import PrUrlModal from "$lib/components/PrUrlModal.svelte";
  import TabStrip from "$lib/components/TabStrip.svelte";
  import Terminal from "$lib/components/Terminal.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import BrowserView from "$lib/components/BrowserView.svelte";
  import AgentOutputView from "$lib/components/AgentOutputView.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { startWindowDrag } from "$lib/windowDrag";
  import { copyToClipboard } from "$lib/clipboard";

  const panels = $derived(app.snapshot?.panels);
  /** No snapshot yet (initial load) OR snapshot has no branch (no repo open). */
  const isEmpty = $derived(
    app.showEmptyState ||
      app.snapshot === null ||
      (app.snapshot.branch === "" && app.snapshot.files.length === 0),
  );

  let drawerHeight = $state(280);

  // --- Right panel resize -------------------------------------------------
  const RIGHT_PANEL_MIN = 280;
  const RIGHT_PANEL_MAX_FRAC = 0.6;
  const RIGHT_PANEL_DEFAULT = 340;
  const RIGHT_PANEL_STORAGE_KEY = "rightPanelWidth";

  let rightPanelWidth = $state(RIGHT_PANEL_DEFAULT);
  let resizingRightPanel = $state(false);

  function clampRightPanelWidth(w: number): number {
    const max = Math.max(RIGHT_PANEL_MIN, window.innerWidth * RIGHT_PANEL_MAX_FRAC);
    return Math.min(max, Math.max(RIGHT_PANEL_MIN, w));
  }

  function onRightPanelResizeStart(e: MouseEvent) {
    e.preventDefault();
    resizingRightPanel = true;
    const startX = e.clientX;
    const startW = rightPanelWidth;

    const onMove = (ev: MouseEvent) => {
      // Dragging left (toward the diff) grows the panel.
      rightPanelWidth = clampRightPanelWidth(startW + (startX - ev.clientX));
    };
    const onUp = () => {
      resizingRightPanel = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      try {
        localStorage.setItem(RIGHT_PANEL_STORAGE_KEY, String(rightPanelWidth));
      } catch {
        /* ignore */
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

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
  const activeTab = $derived(app.snapshot?.tabs?.[activeTabIdx]);
  const activeTabRoot = $derived(
    app.snapshot?.worktrees?.find((w) => w.branch === activeTab?.branch)?.path ??
      activeTab?.repo_root ??
      "",
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
    try {
      const raw = localStorage.getItem(RIGHT_PANEL_STORAGE_KEY);
      const parsed = raw ? Number(raw) : NaN;
      if (Number.isFinite(parsed)) rightPanelWidth = clampRightPanelWidth(parsed);
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

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header
    class="titlebar-drag h-11 border-b border-ink-650 bg-ink-870 flex items-center gap-1 shrink-0 pr-3 pl-3"
    data-tauri-drag-region
    onmousedown={startWindowDrag}
  >
    <!-- left panel + nav buttons -->
    <div class="titlebar-no-drag flex items-center gap-0.5 mr-3 text-ink-300">
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

    <!-- branch chip + worktree controls -->
    <div class="titlebar-no-drag relative flex items-center gap-1 min-w-0">
      <div class="flex items-center gap-2 px-3 py-1 rounded-md bg-ink-700 border border-ink-500 text-sm cursor-default">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
        <span class="text-ink-100 text-sm">{app.snapshot?.branch ?? "Review"}</span>
        {#if app.snapshot?.base}
          <span class="font-mono text-[10px] text-ink-300">{app.snapshot.base}</span>
        {/if}
      </div>
      <button
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-ink-300 hover:text-ink-100 transition-colors"
        title="Copy branch name"
        onclick={() => copyToClipboard(app.snapshot?.branch ?? "")}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
      </button>
      {#if activeTabRoot && activeTab?.kind !== "remote_pr"}
        <button
          class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 text-ink-300 hover:text-ink-100 transition-colors"
          title="Copy repo path"
          onclick={() => copyToClipboard(activeTabRoot)}
        >
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
        </button>
      {/if}
    </div>

    <!-- draggable spacer fills the gap between branch chip and right controls -->
    <div class="flex-1" data-tauri-drag-region></div>

    <!-- right side controls -->
    <div class="titlebar-no-drag flex items-center gap-0.5 text-ink-300">
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

  <div class="flex-1 flex min-h-0 relative">
    {#if app.switching}
      <!-- Immediate loading feedback while a slow command (branch open, tab switch) is in flight. -->
      <div class="absolute inset-0 z-50 flex items-center justify-center bg-ink-900/60 backdrop-blur-sm pointer-events-none">
        <div class="flex items-center gap-2 px-4 py-2 rounded-md bg-ink-800 border border-ink-600 text-fg-2 text-sm">
          <svg class="animate-spin w-4 h-4 text-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/>
          </svg>
          {app.switchingLabel ?? "Switching review..."}
        </div>
      </div>
    {/if}
    {#if app.refreshing}
      <!-- Loading feedback while force_refresh_diff fetches from the remote. -->
      <div class="absolute inset-0 z-50 flex items-center justify-center bg-ink-900/60 backdrop-blur-sm pointer-events-none">
        <div class="flex items-center gap-2 px-4 py-2 rounded-md bg-ink-800 border border-ink-600 text-fg-2 text-sm">
          <svg class="animate-spin w-4 h-4 text-accent" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/>
          </svg>
          Fetching from remote…
        </div>
      </div>
    {/if}

    {#if !browser.open}
      <LeftSidebar collapsed={!panels?.left} />
    {/if}

    <main class="flex-1 flex min-w-0">
      {#if !browser.open && app.mainView === "diff"}
        <FileTree collapsed={!panels?.tree} />
        <DiffView />
      {:else if !browser.open && app.mainView === "agent-output"}
        <AgentOutputView />
      {:else}
        <BrowserView />
      {/if}
    </main>

    {#if panels?.right && !browser.open && app.mainView === "diff"}
      <RightPanel
        ai={app.snapshot?.ai ?? null}
        pr={app.snapshot?.pr ?? null}
        width={rightPanelWidth}
        dragging={resizingRightPanel}
        onResizeStart={onRightPanelResizeStart}
      />
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

  <BackgroundTasks
    tasks={app.snapshot?.background_tasks ?? []}
    avoidRightPanel={!!panels?.right && !browser.open}
    rightPanelWidth={rightPanelWidth}
  />
  <Toast toasts={app.toasts} />
  {#if app.error}
    <div class="fixed bottom-12 left-1/2 -translate-x-1/2 bg-[#3a1a1a] border border-[#f4a3a3] text-[#f4a3a3] text-xs font-mono px-4 py-2 rounded shadow-lg z-50 max-w-[80vw] truncate">
      ⚠ {app.error}
    </div>
  {/if}
  <CommandPalette />
  <AiActionPalette />
  <ExportModal />
  <PrUrlModal />
</div>
{/if}
