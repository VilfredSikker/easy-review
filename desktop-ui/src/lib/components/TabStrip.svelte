<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import type { TabSummary } from "$lib/types";

  interface Props {
    tabs?: TabSummary[];
    active?: number;
    onSelect?: (idx: number) => void;
    onClose?: (idx: number) => void;
    onNew?: () => void;
  }

  let { tabs: tabsProp, active: activeProp, onSelect, onClose, onNew }: Props =
    $props();

  // Default to the live snapshot; props win when supplied (Storybook).
  const tabs = $derived(tabsProp ?? app.snapshot?.tabs ?? []);
  const active = $derived(activeProp ?? app.snapshot?.active_tab ?? 0);
  const canClose = $derived(tabs.length > 1);

  // Drag state. `dragFrom` is the source tab idx; `dropAt` is the insertion
  // marker position (0..tabs.length, where `tabs.length` means after the last
  // tab). Both are reset on dragend/drop.
  let dragFrom = $state<number | null>(null);
  let dropAt = $state<number | null>(null);

  function select(idx: number) {
    browser.open = false;
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
  function newTab() {
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

  // Compute the drop slot relative to a tab element: left half → before, right
  // half → after. Returns the insertion index (0..tabs.length).
  function dropSlotFor(e: DragEvent, idx: number, el: HTMLElement): number {
    const rect = el.getBoundingClientRect();
    const after = e.clientX > rect.left + rect.width / 2;
    return after ? idx + 1 : idx;
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
    // Removal happens before insertion on the backend, so if dropping past the
    // source we must subtract one to land at the visually-intended spot.
    const toIdx = slot > dragFrom ? slot - 1 : slot;
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

  let newTabMenuOpen = $state(false);

  function openNewTabMenu() {
    newTabMenuOpen = !newTabMenuOpen;
  }

  function newReviewTab() {
    newTabMenuOpen = false;
    if (onNew) onNew();
    else app.cmd("new_tab");
  }

  function openBrowserTab() {
    newTabMenuOpen = false;
    browser.open = true;
    // If a review tab was selected and browser wasn't open yet,
    // this brings browser into focus via App.svelte's {#if browser.open} check.
  }
</script>

<div
  class="flex items-center gap-1 h-9 pr-2 border-b border-ink-650 bg-ink-870 overflow-x-auto shrink-0 tabstrip-drag"
  style="padding-left: env(titlebar-area-x, 80px)"
  data-testid="tab-strip"
>
  {#each tabs as tab, i (tab.idx)}
    {#if dragFrom !== null && dropAt === i}
      <div class="w-0.5 h-6 bg-accent rounded-full shrink-0" aria-hidden="true"></div>
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="group flex items-center gap-2 px-3 h-7 rounded-md text-sm cursor-default max-w-[200px] shrink-0 transition-colors {tab.is_active
        ? 'bg-ink-700 border-b-2 border-accent text-ink-100'
        : 'text-ink-300 hover:bg-ink-750'} {dragFrom === i ? 'opacity-50' : ''}"
      onclick={() => select(tab.idx)}
      onauxclick={(e) => handleAuxClick(e, tab.idx)}
      title={`${tab.repo_root} — Click to switch · Middle-click to close`}
      draggable="true"
      ondragstart={(e) => handleDragStart(e, i)}
      ondragover={(e) => handleDragOver(e, i)}
      ondrop={(e) => handleDrop(e, i)}
      ondragend={handleDragEnd}
    >
      {#if tab.kind === "working"}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0">
          <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/>
        </svg>
      {:else if tab.kind === "local_branch"}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0">
          <circle cx="6" cy="6" r="2"/>
          <circle cx="6" cy="18" r="2"/>
          <circle cx="18" cy="12" r="2"/>
          <path d="M6 8v8M8 18h2a4 4 0 0 0 4-4v-2"/>
        </svg>
      {:else}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0">
          <circle cx="18" cy="18" r="3"/>
          <circle cx="6" cy="6" r="3"/>
          <path d="M13 6h3a2 2 0 0 1 2 2v7"/>
          <line x1="6" y1="9" x2="6" y2="21"/>
        </svg>
      {/if}
      <span class="truncate min-w-0">{tab.label}</span>
      {#if canClose}
        <button
          class="opacity-0 group-hover:opacity-100 text-ink-300 hover:text-ink-100 transition-opacity shrink-0 w-4 h-4 flex items-center justify-center"
          onclick={(e) => close(tab.idx, e)}
          title="Close tab"
          aria-label="Close tab {tab.label}"
        >
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M18 6L6 18M6 6l12 12"/>
          </svg>
        </button>
      {/if}
    </div>
  {/each}
  {#if dragFrom !== null && dropAt !== null && dropAt >= tabs.length}
    <div class="w-0.5 h-6 bg-accent rounded-full shrink-0" aria-hidden="true"></div>
  {/if}

  <!-- Browser tab — shown when browser is open -->
  {#if browser.open}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="group flex items-center gap-2 px-3 h-7 rounded-md text-sm cursor-default max-w-[200px] shrink-0 transition-colors bg-ink-700 border-b-2 border-accent text-ink-100"
      title="Browser view"
    >
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0">
        <rect x="3" y="3" width="18" height="18" rx="2"/>
        <path d="M3 9h18"/>
        <circle cx="7" cy="6" r="0.5" fill="currentColor"/>
        <circle cx="10" cy="6" r="0.5" fill="currentColor"/>
      </svg>
      <span class="truncate min-w-0">Browser</span>
      <button
        class="opacity-0 group-hover:opacity-100 text-ink-300 hover:text-ink-100 transition-opacity shrink-0 w-4 h-4 flex items-center justify-center"
        onclick={() => { browser.open = false; browser.annotateMode = false; }}
        title="Close browser"
        aria-label="Close browser tab"
      >
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
          <path d="M18 6L6 18M6 6l12 12"/>
        </svg>
      </button>
    </div>
  {/if}

  <!-- New tab dropdown -->
  <div class="relative shrink-0">
    <button
      class="w-7 h-7 rounded hover:bg-ink-700 flex items-center justify-center text-ink-300 hover:text-ink-100 transition-colors"
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
        <button
          class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
          onclick={newReviewTab}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/>
          </svg>
          Review
        </button>
        <button
          class="w-full text-left px-3 py-2 text-sm text-ink-100 hover:bg-ink-700 flex items-center gap-2"
          onclick={openBrowserTab}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18"/>
          </svg>
          Browser
        </button>
      </div>
    {/if}
  </div>
  <!-- Empty space is window-draggable so the user can move the window from
       the title-bar row, while tabs and buttons remain interactive. -->
  <div class="flex-1 self-stretch tabstrip-drag-region" aria-hidden="true"></div>
</div>

<style>
  /* The strip itself is the title-bar row: by default treat blank space as a
     drag handle. Concrete children opt out via .tabstrip-drag-region inverse
     or their default app-region. */
  .tabstrip-drag :global(.tabstrip-drag-region) {
    -webkit-app-region: drag;
  }
  /* Tabs, buttons, and the new-tab button stay interactive. */
  .tabstrip-drag > :not(.tabstrip-drag-region) {
    -webkit-app-region: no-drag;
  }
</style>
