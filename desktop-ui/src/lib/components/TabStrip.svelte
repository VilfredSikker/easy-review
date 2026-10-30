<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { tabSeen } from "$lib/stores/tabSeen.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { activeJobCount, aggregateJobs, jobsForTab } from "$lib/runningJobs";
  import { arenaRunsAcrossTabs, tabRowModel } from "$lib/tabStripModel";
  import type { TabSummary } from "$lib/types";
  import { destIndexAfterRemove, dropSlot } from "$lib/listReorder";
  import { startWindowDrag } from "$lib/windowDrag";
  import { layoutPanels } from "$lib/stores/layoutPanels.svelte";

  interface Props {
    tabs?: TabSummary[];
    active?: number;
    onSelect?: (idx: number) => void;
    onClose?: (idx: number) => void;
    onNew?: () => void;
    /** When false, render tabs only (Storybook). Default true in the desktop shell. */
    showToolbar?: boolean;
    /** Callback to toggle right panel collapsed/expanded (44px rail vs full). */
    onToggleRightCollapse?: () => void;
    /** Whether the right panel is currently collapsed to 44px rail. */
    rightCollapsed?: boolean;
    /** Tab idxs with AI work in flight; derived from the live snapshot when omitted (Storybook). */
    runningTabs?: ReadonlySet<number>;
  }

  let {
    tabs: tabsProp,
    active: activeProp,
    onSelect,
    onClose,
    onNew,
    showToolbar = true,
    onToggleRightCollapse,
    rightCollapsed = false,
    runningTabs: runningProp,
  }: Props = $props();

  // Default to the live snapshot; props win when supplied (Storybook).
  const tabs = $derived(tabsProp ?? app.snapshot?.tabs ?? []);
  const _active = $derived(activeProp ?? app.snapshot?.active_tab ?? 0);
  const canClose = $derived(tabs.length > 1);
  const runningTabs = $derived.by(() => {
    if (runningProp) return runningProp;
    const jobs = aggregateJobs({
      backgroundTasks: app.snapshot?.background_tasks,
      agentCommands: app.snapshot?.agent_commands,
      arenaRuns: arenaRunsAcrossTabs(arena.summaries, arena.backgroundRuns),
      tabs,
      activeTabIdx: app.snapshot?.active_tab ?? null,
      nowMs: Date.now(),
    });
    return new Set(tabs.filter((t) => activeJobCount(jobsForTab(jobs, t.idx)) > 0).map((t) => t.idx));
  });

  // Drag state. `dragFrom` is the source tab idx; `dropAt` is the insertion
  // marker position (0..tabs.length, where `tabs.length` means after the last
  // tab). Both are reset on dragend/drop.
  let dragFrom = $state<number | null>(null);
  let dropAt = $state<number | null>(null);

  function select(idx: number) {
    if (onSelect) onSelect(idx);
    else app.cmd("select_tab", { idx });
  }
  function close(idx: number, e: Event) {
    e.stopPropagation();
    if (!canClose) return;
    if (onClose) onClose(idx);
    else app.cmd("close_tab", { idx });
  }

  /** Middle-click on a tab closes it (browser-tab convention). */
  function handleAuxClick(e: MouseEvent, idx: number) {
    if (e.button !== 1) return;
    e.preventDefault();
    close(idx, e);
  }
  function _newTab() {
    if (onNew) onNew();
    else app.cmd("new_tab");
  }

  function handleDragStart(e: DragEvent, idx: number) {
    dragFrom = idx;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      // Some browsers refuse to start a drag without data.
      e.dataTransfer.setData("text/plain", String(idx));
    }
  }

  function dropSlotFor(e: DragEvent, idx: number, el: HTMLElement): number {
    const rect = el.getBoundingClientRect();
    return dropSlot(e.clientX, rect.left, rect.width, idx);
  }

  function handleDragOver(e: DragEvent, idx: number) {
    if (dragFrom === null) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropAt = dropSlotFor(e, idx, e.currentTarget as HTMLElement);
  }

  function handleDrop(e: DragEvent, idx: number) {
    if (dragFrom === null) return;
    e.preventDefault();
    const slot = dropSlotFor(e, idx, e.currentTarget as HTMLElement);
    const toIdx = destIndexAfterRemove(dragFrom, slot);
    const fromIdx = dragFrom;
    dragFrom = null;
    dropAt = null;
    if (toIdx !== fromIdx) {
      app.cmd("reorder_tabs", { fromIdx, toIdx });
    }
  }

  function handleDragEnd() {
    dragFrom = null;
    dropAt = null;
  }

  // Mark the active tab as seen whenever it or its change_token changes.
  $effect(() => {
    const activeTab = tabs.find((t) => t.is_active);
    if (activeTab) {
      tabSeen.markSeen(activeTab.idx, activeTab.change_token);
    }
  });

  let newTabMenuOpen = $state(false);

  function openNewTabMenu() {
    newTabMenuOpen = !newTabMenuOpen;
  }

  function newReviewTab() {
    newTabMenuOpen = false;
    if (onNew) onNew();
    else app.cmd("new_tab");
  }

</script>

<!-- Outer wrapper is the window-drag region. Must not be overflow-scrollable
     because WebKit ignores -webkit-app-region: drag inside overflow containers.
     The right padding (pr-3) is always-empty outer space that stays draggable
     even when the tab list is full. The left padding offsets the macOS traffic lights. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="flex items-end h-11 border-b border-line bg-ground shrink-0 tabstrip-drag pr-3 gap-1"
  style="padding-left: env(titlebar-area-x, 80px)"
  data-testid="tab-strip"
  data-tauri-drag-region
  onmousedown={startWindowDrag}
>
  {#if showToolbar}
    <div class="tabstrip-no-drag flex items-center gap-0.5 shrink-0 text-ink-300 self-center">
      <button type="button"
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors {layoutPanels.left ? 'text-accent bg-ink-700' : ''}"
        onclick={() => app.togglePanel("left")}
        title="Toggle left panel [["
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/></svg>
      </button>
    </div>
  {/if}

<!-- Tabs scroll container: overflow here, but explicitly no-drag so tabs stay interactive.
     Use shrink (not flex-1) so leftover space goes to the drag spacer that follows. -->
<div
  class="flex items-end pr-1 min-w-0 overflow-x-auto tabstrip-no-drag {showToolbar ? 'shrink max-w-full' : 'max-w-full'}"
>
  {#each tabs as tab, i (tab.idx)}
    {@const row = tabRowModel(tab, {
      running: runningTabs.has(tab.idx),
      unseen: tabSeen.hasUnseen(tab.idx, tab.change_token),
    })}
    {#if dragFrom !== null && dropAt === i}
      <div class="w-0.5 h-6 mb-1 bg-accent rounded-full shrink-0" aria-hidden="true"></div>
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="wtab group {tab.is_active ? 'on' : ''} {dragFrom === i ? 'opacity-50' : ''}"
      onclick={() => select(tab.idx)}
      onauxclick={(e) => handleAuxClick(e, tab.idx)}
      title={`${tab.repo_root} — Click to switch · Middle-click to close`}
      draggable="true"
      ondragstart={(e) => handleDragStart(e, i)}
      ondragover={(e) => handleDragOver(e, i)}
      ondrop={(e) => handleDrop(e, i)}
      ondragend={handleDragEnd}
    >
      <span class="wtab-b">
        <span class="wtab-ic">
          {#if tab.kind === "working"}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>
            </svg>
          {:else if tab.kind === "local_branch"}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="6" cy="6" r="2"/>
              <circle cx="6" cy="18" r="2"/>
              <circle cx="18" cy="12" r="2"/>
              <path d="M6 8v8M8 18h2a4 4 0 0 0 4-4v-2"/>
            </svg>
          {:else}
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <circle cx="18" cy="18" r="3"/>
              <circle cx="6" cy="6" r="3"/>
              <path d="M13 6h3a2 2 0 0 1 2 2v7"/>
              <line x1="6" y1="9" x2="6" y2="21"/>
            </svg>
          {/if}
        </span>
        <span class="wtab-l">
          {#if row.prefix}<span class="wtab-pr">{row.prefix}</span>&nbsp;{/if}{row.title}
        </span>
        {#if row.mark === "running"}
          <i class="wtab-spin" title="AI running" role="img" aria-label="AI running"></i>
        {:else if row.mark === "stale"}
          <i class="wtab-dot wtab-dot-stale" title="Stale: newer commits upstream" role="img" aria-label="Stale"></i>
        {:else if row.mark === "unseen"}
          <i class="wtab-dot wtab-dot-unseen" title="New changes since last viewed" role="img" aria-label="New changes"></i>
        {/if}
      </span>
      {#if canClose}
        <button type="button"
          class="wtab-x"
          onclick={(e) => close(tab.idx, e)}
          title="Close tab"
          aria-label="Close tab {tab.label}"
        >
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M18 6L6 18M6 6l12 12"/>
          </svg>
        </button>
      {/if}
    </div>
  {/each}
  {#if dragFrom !== null && dropAt !== null && dropAt >= tabs.length}
    <div class="w-0.5 h-6 mb-1 bg-accent rounded-full shrink-0" aria-hidden="true"></div>
  {/if}

  <!-- New tab dropdown -->
  <div class="relative shrink-0">
    <button type="button"
      class="wtab-new flex items-center justify-center"
      onclick={openNewTabMenu}
      title="New tab"
      aria-label="New tab"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M12 5v14M5 12h14"/>
      </svg>
    </button>
    {#if newTabMenuOpen}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="fixed inset-0 z-40" onclick={() => (newTabMenuOpen = false)}></div>
      <div class="absolute left-0 top-full mt-1 z-50 bg-ink-800 border border-ink-500 rounded shadow-xl w-40 py-1">
        <button type="button"
          class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
          onclick={newReviewTab}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/>
          </svg>
          Review
        </button>
      </div>
    {/if}
  </div>
<!-- Close the inner tabs scroll container -->
</div>
  <div class="flex-1 min-w-4 self-stretch" data-tauri-drag-region aria-hidden="true"></div>
  {#if showToolbar && onToggleRightCollapse}
    <div class="tabstrip-no-drag flex items-center gap-0.5 shrink-0 text-ink-300 self-center">
      <button type="button"
        class="w-7 h-7 rounded flex items-center justify-center hover:bg-ink-700 transition-colors {rightCollapsed ? '' : 'text-accent bg-ink-700'}"
        onclick={onToggleRightCollapse}
        title={rightCollapsed ? "Expand right panel" : "Collapse right panel to rail"}
        aria-pressed={!rightCollapsed}
      >
        <!-- Mirrored sidebar icon to indicate collapse direction -->
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="transform: scaleX(-1)"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18"/><path d="M15 8l4 4-4 4"/></svg>
      </button>
    </div>
  {/if}
</div>

<style>
  /* Outer container is the window-drag handle. WebKit requires the drag element
     to not be inside an overflow-scrollable container, so the outer wrapper
     has no overflow and the inner scroll div is explicitly no-drag. */
  .tabstrip-drag {
    -webkit-app-region: drag;
  }
  /* Inner scroll container and all descendants are interactive. */
  .tabstrip-no-drag,
  .tabstrip-no-drag :global(*) {
    -webkit-app-region: no-drag;
  }

  /* Browser-style tab: the active one merges into the page below it. */
  .wtab {
    display: flex;
    align-items: center;
    flex: none;
    max-width: 230px;
    min-width: 120px;
    height: 30px;
    border-radius: 8px 8px 0 0;
    color: var(--color-muted);
    position: relative;
    cursor: default;
  }
  .wtab:hover {
    background: color-mix(in srgb, var(--color-ink-900) 60%, transparent);
    color: var(--color-fg-3);
  }
  .wtab.on {
    background: var(--color-ink-900);
    color: var(--color-fg);
    box-shadow:
      0 1px 0 var(--color-ink-900),
      inset 0 2px 0 var(--color-accent);
  }
  .wtab-b {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0 4px 0 12px;
    font-size: 12.5px;
  }
  .wtab-ic {
    display: inline-grid;
    flex: none;
    color: var(--color-muted);
  }
  .wtab.on .wtab-ic {
    color: var(--color-accent);
  }
  .wtab-l {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .wtab-pr {
    color: var(--color-muted);
  }
  .wtab-x {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    height: 20px;
    margin-right: 5px;
    border-radius: 5px;
    color: var(--color-muted);
    opacity: 0;
  }
  .wtab:hover .wtab-x,
  .wtab.on .wtab-x {
    opacity: 1;
  }
  .wtab-x:hover {
    background: var(--color-ink-700);
    color: var(--color-fg);
  }
  .wtab-new {
    width: 26px;
    height: 26px;
    margin: 0 0 2px 4px;
    border-radius: 6px;
    color: var(--color-muted);
  }
  .wtab-new:hover {
    background: var(--color-ink-700);
    color: var(--color-fg);
  }
  .wtab-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex: none;
  }
  .wtab-dot-stale {
    background: var(--color-warning);
  }
  .wtab-dot-unseen {
    background: var(--color-periwinkle);
  }
  .wtab-spin {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    border: 1.5px solid var(--color-accent);
    border-right-color: transparent;
  }
  @media (prefers-reduced-motion: no-preference) {
    .wtab-spin {
      animation: wtab-spin 0.8s linear infinite;
    }
  }
  @keyframes wtab-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (max-width: 640px) {
    .wtab {
      min-width: 100px;
    }
  }
</style>
