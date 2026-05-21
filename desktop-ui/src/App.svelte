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
  import BranchContextBar from "$lib/components/BranchContextBar.svelte";
  import Terminal from "$lib/components/Terminal.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import BrowserView from "$lib/components/BrowserView.svelte";
  import AgentOutputView from "$lib/components/AgentOutputView.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { browserHide } from "$lib/stores/browserHost";
  const panels = $derived(app.snapshot?.panels);
  /** No snapshot yet, explicit welcome, or no repo/diff and no saved projects. */
  const isEmpty = $derived(
    app.showEmptyState ||
      app.snapshot === null ||
      (app.snapshot.branch === "" &&
        app.snapshot.files.length === 0 &&
        (app.snapshot.projects?.length ?? 0) === 0),
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

  const browserLayout = $derived(browser.layout);
  const showDiff = $derived(browserLayout !== "fullscreen");
  const showBrowser = $derived(browserLayout === "split" || browserLayout === "fullscreen");

  // Belt-and-suspenders: when the browser pane is closed in the UI, park every
  // native child webview. BrowserView's onDestroy also hides, but unmount can
  // race behind the layout command — a stale webview otherwise blocks modals/keys.
  $effect(() => {
    if (!showBrowser) {
      void browserHide();
    }
  });
  let browserSplitRatio = $state(0.45);
  let resizingBrowserSplit = $state(false);
  let splitDebounce: ReturnType<typeof setTimeout> | null = null;

  /** Diff column only shares width with the browser in split mode. */
  const browserSplitWithDiff = $derived(showBrowser && browserLayout === "split");
  const diffColumnFlex = $derived(
    browserSplitWithDiff ? `${browserSplitRatio} 1 0%` : "1 1 0%",
  );

  $effect(() => {
    const r = app.snapshot?.browser?.split_ratio;
    if (r != null && Number.isFinite(r)) browserSplitRatio = r;
  });

  function onBrowserSplitResizeStart(e: MouseEvent) {
    e.preventDefault();
    resizingBrowserSplit = true;
    const row = (e.currentTarget as HTMLElement).parentElement;
    if (!row) return;
    const rect = row.getBoundingClientRect();
    const startX = e.clientX;

    const onMove = (ev: MouseEvent) => {
      const frac = (ev.clientX - rect.left) / rect.width;
      browserSplitRatio = Math.min(0.65, Math.max(0.35, frac));
    };
    const onUp = () => {
      resizingBrowserSplit = false;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (splitDebounce) clearTimeout(splitDebounce);
      splitDebounce = setTimeout(() => {
        void browser.setSplitRatio(browserSplitRatio);
      }, 150);
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

<PrUrlModal />

{#if isEmpty && !app.loading}
  <EmptyState />
{:else}

<div class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden">
  <TabStrip />
  <BranchContextBar />

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

    {#if showDiff}
      <LeftSidebar collapsed={!panels?.left} />
    {/if}

    <main class="flex-1 flex min-w-0 min-h-0">
      <div class="flex flex-1 min-w-0 min-h-0 flex-row">
        {#if showDiff}
          <div
            class="flex flex-1 min-w-0 min-h-0"
            style="flex: {diffColumnFlex};"
          >
            {#if app.mainView === "diff"}
              <FileTree collapsed={!panels?.tree} />
              <DiffView />
            {:else}
              <AgentOutputView />
            {/if}
          </div>
        {/if}
        {#if showBrowser && showDiff}
          <div
            class="w-1 shrink-0 cursor-col-resize hover:bg-accent/40 {resizingBrowserSplit ? 'bg-accent/60' : 'bg-ink-650'}"
            onmousedown={onBrowserSplitResizeStart}
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize browser split"
          ></div>
        {/if}
        {#if showBrowser}
          <div
            class="flex flex-col min-w-0 min-h-0"
            style="flex: {showDiff ? 1 - browserSplitRatio : 1} 1 0%; min-width: 12rem;"
          >
            <BrowserView />
          </div>
        {/if}
      </div>
    </main>

    {#if panels?.right && showDiff && app.mainView === "diff"}
      <RightPanel
        ai={app.snapshot?.ai ?? null}
        pr={app.snapshot?.pr ?? null}
        width={rightPanelWidth}
        dragging={resizingRightPanel}
        onResizeStart={onRightPanelResizeStart}
      />
    {/if}
  </div>

  {#if terminal.open && browserLayout !== "fullscreen"}
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

  {#if showDiff}
    <BottomHints />
  {/if}

  <BackgroundTasks
    tasks={app.snapshot?.background_tasks ?? []}
    avoidRightPanel={!!panels?.right && showDiff}
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
</div>
{/if}
