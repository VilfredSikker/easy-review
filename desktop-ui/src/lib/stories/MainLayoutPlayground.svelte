<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import TabStrip from "$lib/components/TabStrip.svelte";
  import BranchContextBar from "$lib/components/BranchContextBar.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import DiffView from "$lib/components/DiffView.svelte";
  import RightPanel from "$lib/components/RightPanel.svelte";
  import CollapsedRightRail from "$lib/components/CollapsedRightRail.svelte";
  import Toast from "$lib/components/Toast.svelte";
  import BottomHints from "$lib/components/BottomHints.svelte";
  import { richSnapshot } from "$lib/stories/fixtures";
  import type { InboxItemSnapshot } from "$lib/types";

  /**
   * Tauri-invoke safety: Terminal.svelte calls invoke("terminal_spawn") on mount
   * (via dynamic import of @tauri-apps/api/core) and also calls listen() for PTY
   * output events. Both throw in Storybook. Rather than mounting the real Terminal
   * (which would emit an unhandled rejection), we render a lightweight visual
   * stand-in when mockTerminal is true (default in Storybook context).
   *
   * If you set mockTerminal=false you must be running inside a real Tauri window.
   */

  interface Props {
    leftRail?: "expanded" | "collapsed" | "hidden";
    treeRail?: boolean;
    rightRail?: "expanded" | "collapsed" | "hidden";
    terminalOpen?: boolean;
    inboxOpen?: boolean;
    /** When true (default), render a visual terminal stand-in instead of the real
     *  Terminal component so Storybook never calls Tauri's invoke/listen APIs. */
    mockTerminal?: boolean;
  }

  const {
    leftRail = "expanded",
    treeRail = true,
    rightRail = "expanded",
    terminalOpen = false,
    inboxOpen = false,
    mockTerminal = true,
  }: Props = $props();

  // ── inbox fixture items ───────────────────────────────────────────────────
  const inboxFixture: InboxItemSnapshot[] = [
    {
      id: "inbox-1",
      kind: "review_requested",
      severity: "info",
      title: "Review requested: DEV-5008 Show experiment params",
      body: "maria-c requested your review on PR #1090.",
      source: "github",
      target: { pr_number: 1090, project_id: "discovery-platform" },
      created_at_ms: Date.now() - 5 * 60 * 1000,
      read_at_ms: null,
      dedupe_key: "review_requested:1090",
    },
    {
      id: "inbox-2",
      kind: "new_comment",
      severity: "info",
      title: "New comment on PR #1090",
      body: "AI: SchemaMediaProperties is a strict subset — just id, name, kind.",
      source: "github",
      target: { pr_number: 1090, project_id: "discovery-platform" },
      created_at_ms: Date.now() - 12 * 60 * 1000,
      read_at_ms: null,
      dedupe_key: "new_comment:1090:ai-1",
    },
    {
      id: "inbox-3",
      kind: "ci_failed",
      severity: "error",
      title: "CI failed on show-experiment-params",
      body: "2 checks failed. Click to view the failing run.",
      source: "github",
      target: { branch: "show-experiment-params", project_id: "discovery-platform" },
      created_at_ms: Date.now() - 25 * 60 * 1000,
      read_at_ms: Date.now() - 10 * 60 * 1000,
      dedupe_key: "ci_failed:show-experiment-params:run-9912",
    },
  ];

  // ── seed global store ────────────────────────────────────────────────────
  $effect(() => {
    const activeInbox = inboxOpen ? inboxFixture : [];
    app.snapshot = {
      ...richSnapshot,
      panels: {
        left: leftRail !== "hidden",
        tree: treeRail,
        right: rightRail !== "hidden",
      },
      inbox_items: activeInbox,
      inbox_unread_count: activeInbox.filter((i) => i.read_at_ms == null).length,
      inbox_last_refresh_ms: inboxOpen ? Date.now() - 2 * 60 * 1000 : 0,
    };
  });

  // ── sync terminal store to prop ──────────────────────────────────────────
  $effect(() => {
    terminal.open = terminalOpen;
  });

  // ── right panel resize plumbing (mirrors App.svelte) ────────────────────
  const RIGHT_PANEL_DEFAULT = 340;
  let rightPanelWidth = $state(RIGHT_PANEL_DEFAULT);
  let rightPanelCollapsed = $derived(rightRail === "collapsed");

  function onCollapseToggle() {
    // no-op in playground — rightRail state is driven by Storybook args
  }

  function expandRightPanelToTab(_tab: "branch" | "review" | "notes") {
    // no-op in playground — use Storybook Controls to change rightRail
  }

  // ── toast control bar ────────────────────────────────────────────────────
  function fireSuccessToast() {
    app.showToast("success", "Changes committed to show-experiment-params.");
  }
  function fireInfoToast() {
    app.showToast("info", "AI review is running in the background.");
  }
  function fireWarnToast() {
    app.showToast("warn", "Diff is stale — the branch was updated 3 minutes ago.");
  }
  function fireErrorToast() {
    app.showToast(
      "error",
      "Failed to push to origin/show-experiment-params: remote rejected (pre-receive hook declined). Check CI rules and try again.",
      10_000,
      {
        persist: true,
        action: {
          label: "Retry",
          onClick: () => app.showToast("info", "Retrying push…"),
        },
      },
    );
  }

</script>

<div class="h-screen flex flex-col bg-ink-900 text-ink-50 overflow-hidden">
  <!-- Tab strip -->
  <TabStrip
    showToolbar={true}
    onToggleRightCollapse={onCollapseToggle}
    rightCollapsed={rightPanelCollapsed}
  />

  <!-- Branch context bar -->
  <BranchContextBar />

  <!-- Body row -->
  <div class="flex-1 flex min-h-0 relative">
    <!-- Left sidebar -->
    {#if leftRail !== "hidden"}
      <LeftSidebar collapsed={leftRail === "collapsed"} />
    {/if}

    <!-- Main content area -->
    <main class="flex-1 flex min-w-0 min-h-0">
      <FileTree collapsed={!treeRail} />
      <DiffView />
    </main>

    <!-- Right panel / collapsed rail -->
    {#if rightRail === "expanded"}
      <RightPanel
        ai={app.snapshot?.ai ?? null}
        pr={app.snapshot?.pr ?? null}
        width={rightPanelWidth}
        dragging={false}
        onResizeStart={undefined}
        onCollapseToggle={onCollapseToggle}
      />
    {:else if rightRail === "collapsed"}
      <CollapsedRightRail
        ai={app.snapshot?.ai ?? null}
        onExpand={expandRightPanelToTab}
      />
    {/if}
  </div>

  <!-- Terminal drawer -->
  {#if terminalOpen}
    <div class="relative border-t border-hairline bg-ink-900 shrink-0" style="height: 240px">
      {#if mockTerminal}
        <!-- Visual stand-in: avoids Tauri invoke/listen calls that throw in Storybook -->
        <div class="w-full h-full flex flex-col bg-[#0e0e0e] overflow-hidden">
          <div class="h-6 shrink-0 border-b border-[#1f1f1f] bg-[#131313] flex items-center gap-2 px-2 text-[11px] font-mono text-[#999]">
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0">
              <polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" />
            </svg>
            <span class="truncate">{app.snapshot?.branch ?? "—"}</span>
            <span class="ml-2 text-[10px] text-[#555]">[story mode — real Terminal requires Tauri]</span>
            <div class="flex-1"></div>
            <button
              type="button"
              class="w-5 h-5 flex items-center justify-center rounded text-[#666] hover:text-[#ccc]"
              onclick={() => (terminal.open = false)}
              title="Close terminal"
              aria-label="Close terminal"
            >
              <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
          <div class="flex-1 p-3 font-mono text-[12px] text-[#9ad79a] overflow-hidden">
            <div class="text-[#7aa8e6]">~/Projects/discovery-platform (show-experiment-params)</div>
            <div class="mt-1 flex items-center gap-1">
              <span class="text-[#ff6a3d]">$</span>
              <span class="text-[#e6e6e6]">git status</span>
            </div>
            <div class="mt-1 text-[#5e5e5e]">On branch show-experiment-params</div>
            <div class="text-[#5e5e5e]">nothing to commit, working tree clean</div>
            <div class="mt-2 flex items-center gap-1">
              <span class="text-[#ff6a3d]">$</span>
              <span class="inline-block w-2 h-4 bg-[#ff6a3d] animate-pulse"></span>
            </div>
          </div>
        </div>
      {/if}
    </div>
  {/if}

  {#if !terminalOpen}
    <BottomHints />
  {/if}

  <!-- Toast stack -->
  <Toast toasts={app.toasts} />

  <!-- Floating toast control bar (bottom-left, fixed) -->
  <div class="fixed bottom-4 left-4 z-50 flex items-center gap-1.5 bg-ink-800/90 backdrop-blur-sm border border-hairline rounded-lg px-2 py-1.5 shadow-lg">
    <span class="text-[10px] text-muted font-mono mr-1">Toasts:</span>
    <button
      type="button"
      onclick={fireSuccessToast}
      class="px-2 py-0.5 rounded text-[11px] font-medium bg-add-fg/20 text-add-fg hover:bg-add-fg/30 transition-colors"
    >success</button>
    <button
      type="button"
      onclick={fireInfoToast}
      class="px-2 py-0.5 rounded text-[11px] font-medium bg-accent/20 text-accent hover:bg-accent/30 transition-colors"
    >info</button>
    <button
      type="button"
      onclick={fireWarnToast}
      class="px-2 py-0.5 rounded text-[11px] font-medium bg-risk-med/20 text-risk-med hover:bg-risk-med/30 transition-colors"
    >warn</button>
    <button
      type="button"
      onclick={fireErrorToast}
      class="px-2 py-0.5 rounded text-[11px] font-medium bg-del-fg/20 text-del-fg hover:bg-del-fg/30 transition-colors"
    >error + action</button>
  </div>
</div>
