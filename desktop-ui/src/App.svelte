<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { initKeyboard } from "$lib/stores/keyboard";
  import AppMark from "$lib/components/AppMark.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import CollapsedRightRail from "$lib/components/CollapsedRightRail.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import BackgroundTasks from "$lib/components/BackgroundTasks.svelte";
  import CommandPalette from "$lib/components/CommandPalette.svelte";
  import AiActionPalette from "$lib/components/AiActionPalette.svelte";
  import AiReviewFilesModal from "$lib/components/AiReviewFilesModal.svelte";
  import ProfessorFocusModal from "$lib/components/ProfessorFocusModal.svelte";
  import ArenaLauncher from "$lib/components/arena/ArenaLauncher.svelte";
  import ArenaRunningPanel from "$lib/components/arena/ArenaRunningPanel.svelte";
  import ArenaOverlay from "$lib/components/arena/ArenaOverlay.svelte";
  import EmptyState from "$lib/components/EmptyState.svelte";
  import PrUrlModal from "$lib/components/PrUrlModal.svelte";
  import TabStrip from "$lib/components/TabStrip.svelte";
  import BranchContextBar from "$lib/components/BranchContextBar.svelte";
  import Terminal from "$lib/components/Terminal.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { rightRail } from "$lib/stores/rightRail.svelte";
  import BrowserView from "$lib/components/BrowserView.svelte";
  import AgentOutputView from "$lib/components/AgentOutputView.svelte";
  import ExportReviewView from "$lib/components/ExportReviewView.svelte";
  import SettingsPage from "$lib/components/settings/SettingsPage.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { browserHide } from "$lib/stores/browserHost";
  import { installExternalLinkGuard } from "$lib/openExternalUrl";
  import { startWindowDrag } from "$lib/windowDrag";
  import { applyTheme } from "$lib/themes";
  const panels = $derived(app.snapshot?.panels);

  // Follow the configured theme (`display.theme`, same as the TUI): override
  // the CSS vars on the root element and switch the Shiki syntax theme.
  $effect(() => {
    if (!app.snapshot) return;
    const theme = applyTheme(app.snapshot.theme);
    if (app.currentSyntaxTheme !== theme.syntaxThemeId) {
      app.currentSyntaxTheme = theme.syntaxThemeId;
    }
  });

  /** True when a loaded snapshot confirms there is no repo/diff and no saved projects. */
  const naturalEmpty = $derived(
    app.snapshot !== null &&
      app.snapshot.branch === "" &&
      app.snapshot.files.length === 0 &&
      (app.snapshot.projects?.length ?? 0) === 0,
  );
  const hasOpenTabs = $derived((app.snapshot?.tabs?.length ?? 0) > 0);
  const explicitFullWelcome = $derived(app.showEmptyState && app.snapshot !== null && !hasOpenTabs);
  const showFullWelcome = $derived((naturalEmpty || explicitFullWelcome) && !app.loading);
  const snapshotPending = $derived(app.snapshot === null && (!app.initialLoadDone || app.loading));
  const snapshotUnavailable = $derived(app.snapshot === null && app.initialLoadDone && !app.loading);
  /** "New review" while tabs are open — overlay on top of the review shell. */
  const showWelcomeOverlay = $derived(app.showEmptyState && hasOpenTabs && !naturalEmpty);

  let drawerHeight = $state(280);

  // --- Right panel resize -------------------------------------------------
  const RIGHT_PANEL_MIN = 280;
  const RIGHT_PANEL_MAX_FRAC = 0.6;
  const RIGHT_PANEL_DEFAULT = 340;
  const RIGHT_PANEL_STORAGE_KEY = "rightPanelWidth";

  let rightPanelWidth = $state(RIGHT_PANEL_DEFAULT);
  let resizingRightPanel = $state(false);

  function expandRightPanelToTab(tab: "branch" | "review" | "notes") {
    rightRail.expand();
    try {
      localStorage.setItem("rightPanelActiveTab", tab);
    } catch { /* ignore */ }
  }

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
  let terminalRefitToken = $state(0);

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
      terminalRefitToken += 1;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  const browserLayout = $derived(browser.layout);
  const isSettingsView = $derived(app.mainView === "settings");
  const showDiff = $derived(!isSettingsView && browserLayout !== "fullscreen");
  const showBrowser = $derived(
    !isSettingsView && (browserLayout === "split" || browserLayout === "fullscreen"),
  );

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
    if (resizingBrowserSplit) return;
    const r = app.snapshot?.browser?.split_ratio;
    if (r == null || !Number.isFinite(r)) return;
    if (Math.abs(browserSplitRatio - r) < 0.001) return;
    browserSplitRatio = r;
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

  $effect(() => {
    app.snapshot;
    arena.syncFromSnapshot();
  });

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
    void import("$lib/dev/log").then(({ initDevLog }) => initDevLog());
    app.load().then(() => app.startPolling());
    const cleanupKeyboard = initKeyboard();
    const cleanupLinkGuard = installExternalLinkGuard();
    const unlistenArena = listen<number>("er://revision", () => {
      arena.onRevision();
    });
    return () => {
      cleanupKeyboard();
      cleanupLinkGuard();
      app.stopPolling();
      void unlistenArena.then((fn) => fn());
    };
  });
</script>

<PrUrlModal />

{#if showFullWelcome}
  <EmptyState />
{:else if snapshotPending || snapshotUnavailable}
  <div class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="titlebar-drag h-11 px-4 border-b border-hairline bg-ink-870 flex items-center gap-2 shrink-0"
      style="padding-left: env(titlebar-area-x, 80px)"
      data-tauri-drag-region
      onmousedown={startWindowDrag}
    >
      <AppMark size={20} />
      <span class="text-sm">Easy Review</span>
    </div>
    <main class="flex-1 flex items-center justify-center p-8">
      <div class="max-w-md text-center">
        <div class="mx-auto mb-4 h-8 w-8 rounded-full border-2 border-ink-600 border-t-accent {snapshotPending ? 'animate-spin' : ''}"></div>
        <h1 class="text-base font-medium text-fg-1">
          {snapshotPending ? "Loading review..." : "Could not load review state"}
        </h1>
        <p class="mt-2 text-sm text-fg-3">
          {snapshotPending
            ? "Waiting for the desktop backend to return the current snapshot."
            : (app.error ?? "The desktop backend did not return a snapshot.")}
        </p>
      </div>
    </main>
    <Toast toasts={app.toasts} />
  </div>
{:else}

<div
  class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden"
  style="--shell-bottom-chrome: {terminal.open ? '12px' : '28px'}"
>
  <TabStrip onToggleRightCollapse={rightRail.toggle} rightCollapsed={rightRail.collapsed} />
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
      {#if isSettingsView}
        <SettingsPage onBack={() => app.setMainView("diff")} />
      {:else}
      <div class="flex flex-1 min-w-0 min-h-0 flex-row">
        {#if showDiff}
          <div
            class="flex flex-1 min-w-0 min-h-0"
            style="flex: {diffColumnFlex};"
          >
            {#if app.mainView === "diff"}
              <FileTree collapsed={!panels?.tree} />
              <DiffView />
            {:else if app.mainView === "agent-output"}
              <AgentOutputView />
            {:else}
              <ExportReviewView />
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
      {/if}
    </main>

    {#if showDiff && app.mainView === "diff"}
      {#if rightRail.collapsed}
        <CollapsedRightRail
          ai={app.snapshot?.ai ?? null}
          onExpand={expandRightPanelToTab}
        />
      {:else}
        <RightPanel
          ai={app.snapshot?.ai ?? null}
          pr={app.snapshot?.pr ?? null}
          width={rightPanelWidth}
          dragging={resizingRightPanel}
          onResizeStart={onRightPanelResizeStart}
          onCollapseToggle={rightRail.toggle}
        />
      {/if}
    {/if}
  </div>

  {#if terminal.everOpened && browserLayout !== "fullscreen"}
    <div
      class="relative border-t border-hairline bg-ink-900 shrink-0 overflow-hidden"
      style="height: {terminal.open ? drawerHeight : 0}px"
    >
      <!--
        4px drag handle along the top edge. Sits above the border, captures
        mousedown to start resizing. While dragging we set `cursor: ns-resize`
        on body via the `dragging` class so the cursor doesn't flicker when
        the pointer briefly leaves the handle.
      -->
      <div
        class="absolute -top-[2px] left-0 right-0 h-1 cursor-row-resize z-10 hover:bg-accent/40 {dragging ? 'bg-accent/60' : ''}"
        onmousedown={onResizeStart}
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize terminal drawer"
      ></div>
      <Terminal
        sessionId={`tab-${activeTabIdx}`}
        cwd={activeTabRoot}
        visible={terminal.open}
        refitToken={terminalRefitToken}
      />
    </div>
  {/if}

  <BackgroundTasks
    tasks={app.snapshot?.background_tasks ?? []}
    avoidRightPanel={showDiff && app.mainView === "diff"}
    rightPanelWidth={rightRail.collapsed ? 44 : rightPanelWidth}
  />
  <Toast toasts={app.toasts} />
  {#if app.error}
    <div class="fixed bottom-12 left-1/2 -translate-x-1/2 bg-[color-mix(in_srgb,var(--color-error)_18%,var(--color-bg))] border border-error/70 text-error text-xs font-mono px-4 py-2 rounded shadow-lg z-50 max-w-[80vw] truncate">
      ⚠ {app.error}
    </div>
  {/if}
  <CommandPalette />
  <AiActionPalette />
  <AiReviewFilesModal />
  <ProfessorFocusModal />

  <ArenaLauncher
    open={arena.launcherOpen}
    onClose={() => arena.closeLauncher()}
    preset={arena.lastConfig?.reviewers}
  />
  <ArenaRunningPanel
    open={arena.runningOpen}
    minimized={arena.runningMinimized}
    config={arena.lastConfig}
    liveRuns={arena.liveRuns}
    liveRunStates={arena.liveRunStates}
    snapshot={arena.liveSnapshot}
    progress={arena.progress}
    startedAt={arena.runStartedAt}
    onMinimize={() => arena.minimizeRunning()}
    onRestore={() => arena.restoreRunning()}
    onCancel={() => void arena.cancelRun()}
    onComplete={() => {
      /* Terminal handling (Review tab vs Arena overlay) lives in arena.svelte refreshLiveRun. */
    }}
  />
  {#if arena.overlayOpen && arena.overlaySnapshot}
    <ArenaOverlay
      open={true}
      snapshot={arena.overlaySnapshot}
      bind:layoutMode={arena.layoutMode}
      onClose={() => arena.closeOverlay()}
      onNewRun={() => {
        arena.closeOverlay();
        arena.openLauncher();
      }}
    />
  {/if}

  {#if showWelcomeOverlay}
    <div class="fixed inset-0 z-[200]">
      <EmptyState overlay />
    </div>
  {/if}
</div>
{/if}
