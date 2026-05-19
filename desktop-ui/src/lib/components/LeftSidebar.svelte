<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { InboxItemSnapshot, ProjectSnapshot, PrInfo } from "$lib/types";
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";

  interface PinnedItem {
    id: string;
    title: string;
    age: string;
  }

  interface Props {
    /** When true, render the narrow icon-only rail (mock lines 239–266). */
    collapsed?: boolean;
    /** Override pinned items (currently no snapshot field — story-time injection). */
    pinnedOverride?: PinnedItem[];
  }
  const { collapsed = false, pinnedOverride }: Props = $props();

  const snapshot = $derived(app.snapshot);
  const worktrees = $derived(snapshot?.worktrees ?? []);
  const projects = $derived<ProjectSnapshot[]>(snapshot?.projects ?? []);
  const inboxItems = $derived<InboxItemSnapshot[]>(snapshot?.inbox_items ?? []);
  const inboxUnreadCount = $derived<number>(snapshot?.inbox_unread_count ?? 0);
  const inboxLastRefreshMs = $derived<number>(snapshot?.inbox_last_refresh_ms ?? 0);
  const loadingPrList = $derived(snapshot?.bg_loading?.pr_list ?? false);
  const activeTab = $derived(snapshot?.tabs?.find((t) => t.is_active) ?? null);

  function projectFromPath(path: string): string {
    const segments = path.split("/").filter(Boolean);
    return segments[segments.length - 2] ?? segments[segments.length - 1] ?? "project";
  }

  const currentWorktree = $derived(worktrees.find((w) => w.is_current));
  const fallbackProjectName = $derived(
    currentWorktree ? projectFromPath(currentWorktree.path) : "current",
  );

  const pinned = $derived<PinnedItem[]>(pinnedOverride ?? []);
  const inboxVisible = $derived(
    [...inboxItems].sort((a, b) => {
      const aUnread = a.read_at_ms == null ? 0 : 1;
      const bUnread = b.read_at_ms == null ? 0 : 1;
      if (aUnread !== bUnread) return aUnread - bUnread;
      return b.created_at_ms - a.created_at_ms;
    }).slice(0, 20),
  );
  const latestInboxMessage = $derived(inboxVisible[0] ?? null);

  let settingsOpen = $state(false);
  let inboxPopoverOpen = $state(false);
  let selectedInboxMessage = $state<InboxItemSnapshot | null>(null);
  let expandedProject = $state<string | null>(null);
  let pendingBranchKey = $state<string | null>(null);
  let pendingPrKey = $state<string | null>(null);
  let prRevealCountByProject = $state<Record<string, number>>({});
  let sidebarSearch = $state("");

  const sidebarSearchNeedle = $derived(sidebarSearch.trim().toLowerCase());
  const searchActive = $derived(sidebarSearchNeedle.length > 0);
  const matchesSearch = (v: string): boolean => v.toLowerCase().includes(sidebarSearchNeedle);
  const filteredProjects = $derived(
    !sidebarSearchNeedle
      ? projects
      : projects.filter((project) => {
          if (matchesSearch(project.name)) return true;
          if (project.local_branches.some((br) => matchesSearch(br.name))) return true;
          if (project.my_prs.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          if (project.prs_to_review.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          if (project.recently_merged.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          return false;
        }),
  );

  // Branch-picker state for the project header "+" button.
  let addingTo = $state<string | null>(null);
  let availableBranches = $state<string[]>([]);
  let pickerLoading = $state(false);

  async function openBranchPicker(projectId: string, e: MouseEvent) {
    e.stopPropagation();
    if (addingTo === projectId) {
      addingTo = null;
      return;
    }
    addingTo = projectId;
    pickerLoading = true;
    try {
      availableBranches = await invoke<string[]>("list_available_branches", { projectId });
    } catch (err) {
      app.pushLog("error", "list_available_branches", String(err));
      availableBranches = [];
    } finally {
      pickerLoading = false;
    }
  }

  async function pickBranch(projectId: string, name: string) {
    await app.cmd("add_tracked_branch", { projectId, name });
    addingTo = null;
  }

  function closeBranchPicker() {
    addingTo = null;
  }

  function onSettingsKey(e: KeyboardEvent) {
    if (e.key === "Escape") settingsOpen = false;
    if (e.key === "Escape") {
      inboxPopoverOpen = false;
      selectedInboxMessage = null;
    }
  }

  function openInboxPopover() {
    inboxPopoverOpen = true;
  }

  function closeInboxPopover() {
    inboxPopoverOpen = false;
  }

  function openInboxMessageModal(item: InboxItemSnapshot) {
    selectedInboxMessage = item;
    app.cmd("mark_inbox_item_read", { id: item.id });
  }

  function closeInboxMessageModal() {
    selectedInboxMessage = null;
  }

  function projectBadge(p: ProjectSnapshot): number {
    return p.local_branches.length + p.my_prs.length + p.prs_to_review.length + p.recently_merged.length;
  }

  function formatCacheAge(ms?: number | null): string {
    if (ms == null || ms < 0) return "";
    const mins = Math.floor(ms / 60_000);
    if (mins < 1) return "<1m";
    if (mins < 60) return `${mins}m`;
    const hrs = Math.floor(mins / 60);
    return `${hrs}h`;
  }

  function formatInboxUpdated(ms: number): string {
    if (!ms || ms <= 0) return "never";
    const delta = Date.now() - ms;
    if (delta < 60_000) return "just now";
    const mins = Math.floor(delta / 60_000);
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    return `${hrs}h ago`;
  }

  function prIconColor(pr: PrInfo): string {
    if (pr.state === "MERGED") return "text-purple-400";
    if (pr.review_decision === "CHANGES_REQUESTED") return "text-del-fg";
    if (pr.review_decision === "APPROVED") return "text-add-fg";
    if (pr.is_draft) return "text-muted";
    return "text-fg-3";
  }

  function isProjectOpen(p: ProjectSnapshot): boolean {
    if (sidebarSearchNeedle) return true;
    return p.is_active || expandedProject === p.id;
  }

  function visibleBranches(project: ProjectSnapshot) {
    return searchActive ? project.local_branches.filter((br) => matchesSearch(br.name)) : project.local_branches;
  }

  function visibleMyPrs(project: ProjectSnapshot) {
    return searchActive
      ? project.my_prs.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : project.my_prs;
  }

  function visibleToReviewPrs(project: ProjectSnapshot) {
    return searchActive
      ? project.prs_to_review.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : project.prs_to_review;
  }

  function visibleRecentlyMergedPrs(project: ProjectSnapshot) {
    return searchActive
      ? project.recently_merged.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : project.recently_merged;
  }

  function toggleProject(p: ProjectSnapshot) {
    expandedProject = expandedProject === p.id ? null : p.id;
  }

  function branchRowAction(projectId: string, name: string, e: MouseEvent) {
    e.stopPropagation();
    app.cmd("remove_tracked_branch", { projectId, name });
  }

  /** Plain click replaces the active tab. Cmd/Ctrl-click or middle-click opens
   * a new tab. (Inverse of the previous behavior — power users use modifiers
   * when they want to keep the current tab around.) */
  function shouldReplaceTab(e: MouseEvent): boolean {
    return !(e.metaKey || e.ctrlKey || e.button === 1);
  }

  function nextAnimationFrame(): Promise<void> {
    return new Promise((resolve) => {
      if (typeof requestAnimationFrame === "function") {
        requestAnimationFrame(() => resolve());
      } else {
        setTimeout(resolve, 0);
      }
    });
  }

  async function yieldForPendingPaint() {
    await tick();
    await nextAnimationFrame();
  }

  async function openBranch(projectId: string, name: string, e: MouseEvent) {
    const branchKey = `${projectId}:${name}`;
    if (pendingBranchKey === branchKey) return;
    pendingBranchKey = branchKey;
    try {
      await yieldForPendingPaint();
      await app.cmd("open_local_branch", {
        projectId,
        name,
        replace: shouldReplaceTab(e),
      });
    } finally {
      if (pendingBranchKey === branchKey) pendingBranchKey = null;
    }
  }

  async function openPr(projectId: string, prNumber: number, _headRef: string, e: MouseEvent, hint?: PrInfo) {
    const prKey = `${projectId}:${prNumber}`;
    if (pendingPrKey === prKey) return;
    pendingPrKey = prKey;
    // Clear any pending hover-prefetch timer for this PR — the click supersedes it.
    cancelPrPrefetch(projectId, prNumber);
    try {
      await yieldForPendingPaint();
      await app.cmd("open_pr_review", {
        projectId,
        prNumber,
        replace: shouldReplaceTab(e),
        hint: hint ? buildPrHint(hint) : undefined,
      });
    } finally {
      if (pendingPrKey === prKey) pendingPrKey = null;
    }
  }

  // ── PR hover-prefetch ──
  // After a short debounce on hover, kick a background `prefetch_pr_open` to
  // warm the diff cache so the click feels instant. If the cursor leaves
  // before the debounce fires, the timer is cleared and no fetch starts.
  const PR_HOVER_PREFETCH_DELAY_MS = 150;
  const prPrefetchTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function buildPrHint(pr: PrInfo) {
    return {
      baseRef: pr.base_ref,
      headRef: pr.head_ref,
      headOid: pr.head_oid,
      updatedAt: pr.updated_at,
      title: pr.title,
      author: pr.author,
    };
  }

  function schedulePrPrefetch(projectId: string, pr: PrInfo) {
    // No useful hint to send → skip; the open path falls back to the slow
    // synchronous gh-pr-view round-trip anyway.
    if (!pr.head_oid || !pr.base_ref) return;
    const key = `${projectId}:${pr.number}`;
    if (prPrefetchTimers.has(key)) return;
    const timer = setTimeout(() => {
      prPrefetchTimers.delete(key);
      // Bypass app.cmd() — that assigns the return value to app.snapshot, and
      // prefetch_pr_open returns () which would null out the snapshot and
      // render the empty page. Fire-and-forget invoke is correct here.
      invoke("prefetch_pr_open", {
        projectId,
        prNumber: pr.number,
        hint: buildPrHint(pr),
      }).catch(() => {
        // Background fetch — failure is logged in Rust, nothing to do here.
      });
    }, PR_HOVER_PREFETCH_DELAY_MS);
    prPrefetchTimers.set(key, timer);
  }

  function cancelPrPrefetch(projectId: string, prNumber: number) {
    const key = `${projectId}:${prNumber}`;
    const timer = prPrefetchTimers.get(key);
    if (timer !== undefined) {
      clearTimeout(timer);
      prPrefetchTimers.delete(key);
    }
  }

  function visibleToReviewCount(projectId: string): number {
    return prRevealCountByProject[projectId] ?? 5;
  }

  function revealMoreToReview(projectId: string) {
    const next = visibleToReviewCount(projectId) + 5;
    prRevealCountByProject = { ...prRevealCountByProject, [projectId]: next };
  }

</script>

{#if collapsed}
  <!-- Collapsed rail -->
  <aside class="w-11 bg-surface border-r border-hairline shrink-0 flex flex-col items-center py-3 gap-2 transition-[width] duration-200">
    <button
      onclick={() => app.togglePanel("left")}
      title="Expand sidebar"
      aria-label="Expand left sidebar"
      class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
    </button>
    <button
      onclick={() => (app.showEmptyState = true)}
      title="New review"
      aria-label="New review"
      class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
    </button>
    <button title="Search" aria-label="Search" class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
    </button>
    <div class="h-px w-5 bg-hairline my-1"></div>
    <button title={fallbackProjectName} aria-label={fallbackProjectName} class="w-7 h-7 rounded bg-hover flex items-center justify-center text-accent">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7l9-5 9 5v10l-9 5-9-5V7z"/></svg>
    </button>
    <div class="mt-auto">
      <button title="Settings" aria-label="Settings" class="w-7 h-7 rounded bg-accent flex items-center justify-center text-black text-[10px] font-bold">er</button>
    </div>
  </aside>
{:else}

<aside class="w-60 bg-surface border-r border-hairline shrink-0 flex flex-col h-full overflow-hidden transition-[width] duration-200">
  <!-- Scrollable content; Settings footer is fixed at bottom outside this wrapper. -->
  <div class="flex-1 overflow-y-auto min-h-0">
  <!-- Top actions -->
  <div class="px-2 pt-2 pb-2 space-y-0.5">
    <button
      onclick={() => (app.showEmptyState = true)}
      class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-2"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
      <span>New review</span>
    </button>
    <button
      onclick={() => { document.querySelector<HTMLInputElement>('[data-left-sidebar-search-input]')?.focus(); }}
      class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
      <span>Search</span>
      <span class="kbd ml-auto">⌘P</span>
    </button>
    <div class="px-2 pt-1">
      <input
        data-left-sidebar-search-input
        value={sidebarSearch}
        oninput={(e) => (sidebarSearch = (e.currentTarget as HTMLInputElement).value)}
        class="w-full bg-surface border border-hairline rounded-md px-2 py-1.5 text-sm text-fg-2 placeholder:text-muted outline-none"
        placeholder="Search projects, branches, PRs…"
      />
    </div>
  </div>

  {#if pinned.length > 0}
    <div class="px-2 pt-2 pb-2">
      <div class="text-[10px] uppercase tracking-wider text-muted mb-1 px-2">Pinned</div>
      <div class="space-y-0.5">
        {#each pinned as item (item.id)}
          <div class="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-2">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent"><path d="M12 17v5M9 10.76V19l3 2 3-2v-8.24"/><path d="M3 7l9-5 9 5"/></svg>
            <span class="truncate">{item.title}</span>
            <span class="font-mono text-[10px] text-muted ml-auto">{item.age}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Inbox -->
  <div class="px-2 pt-2 pb-2">
    <div class="flex items-center px-2 mb-1">
      <button
        type="button"
        onclick={openInboxPopover}
        class="text-[10px] uppercase tracking-wider text-muted hover:text-fg"
      >
        Inbox
      </button>
      {#if inboxUnreadCount > 0}
        <span class="ml-1 text-[10px] font-mono text-amber-300">{inboxUnreadCount}</span>
      {/if}
      <button
        type="button"
        onclick={() => app.cmd("refresh_notifications")}
        class="ml-auto text-[10px] text-muted hover:text-fg"
      >↻</button>
    </div>
    <button
      type="button"
      onclick={openInboxPopover}
      class="w-full text-left px-2 py-1.5 rounded-md hover:bg-hover text-xs"
    >
      {#if latestInboxMessage}
        <div class="flex items-center gap-1.5">
          <span class={latestInboxMessage.severity === "error" ? "text-del-fg" : latestInboxMessage.severity === "warning" ? "text-amber-300" : "text-muted"}>●</span>
          <span class="truncate {latestInboxMessage.read_at_ms == null ? 'text-fg-2' : 'text-fg-3'}">{latestInboxMessage.title}</span>
        </div>
        {#if latestInboxMessage.body}
          <div class="truncate text-[10px] text-muted ml-3">{latestInboxMessage.body}</div>
        {/if}
      {:else}
        <div class="text-xs text-muted">No notifications</div>
      {/if}
    </button>
  </div>

  {#if inboxPopoverOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fixed inset-0 z-[200]" onclick={closeInboxPopover}></div>
    <div class="absolute left-2 top-28 z-[201] w-64 h-[300px] rounded-md border border-border bg-card shadow-xl">
      <div class="px-3 py-2 text-xs text-muted border-b border-hairline flex items-center">
        <span>Inbox · Updated {formatInboxUpdated(inboxLastRefreshMs)}</span>
        <button
          type="button"
          onclick={() => app.cmd("mark_all_inbox_read")}
          class="ml-auto text-[10px] text-muted hover:text-fg"
        >Read all</button>
        <button
          type="button"
          onclick={() => app.cmd("clear_read_inbox_items")}
          class="ml-2 text-[10px] text-muted hover:text-fg"
        >Clear read</button>
      </div>
      <div class="h-[255px] overflow-y-auto p-1 space-y-0.5">
        {#if inboxVisible.length === 0}
          <div class="px-2 py-2 text-xs text-muted">No notifications</div>
        {:else}
          {#each inboxVisible as item (item.id)}
            <button
              type="button"
              onclick={() => openInboxMessageModal(item)}
              class="w-full text-left px-2 py-1.5 rounded-md hover:bg-hover text-xs {item.read_at_ms == null ? 'text-fg-2' : 'text-fg-3'}"
            >
              <div class="flex items-center gap-1.5">
                <span class={item.severity === "error" ? "text-del-fg" : item.severity === "warning" ? "text-amber-300" : "text-muted"}>●</span>
                <span class="truncate block min-w-0 flex-1">{item.title}</span>
              </div>
              {#if item.body}
                <div class="truncate block min-w-0 text-[10px] text-muted ml-3">{item.body}</div>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    </div>
  {/if}

  {#if selectedInboxMessage}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fixed inset-0 z-[250] bg-black/60" onclick={closeInboxMessageModal}></div>
    <div
      class="fixed inset-0 z-[251] flex items-center justify-center p-6"
      onclick={(e) => {
        if (e.target === e.currentTarget) closeInboxMessageModal();
      }}
    >
      <div class="w-full max-w-2xl rounded-lg border border-border bg-surface shadow-xl">
        <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
          <span class={selectedInboxMessage.severity === "error" ? "text-del-fg" : selectedInboxMessage.severity === "warning" ? "text-amber-300" : "text-muted"}>●</span>
          <div class="text-sm text-fg-1 truncate">{selectedInboxMessage.title}</div>
          <button class="ml-auto text-muted hover:text-fg px-2" onclick={closeInboxMessageModal}>×</button>
        </div>
        <div class="px-4 py-3 text-sm text-fg-2 whitespace-pre-wrap break-words max-h-[50vh] overflow-y-auto">
          {selectedInboxMessage.body || "(No message body)"}
        </div>
        <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2">
          <button class="px-3 py-1.5 rounded border border-border text-sm text-fg-2 hover:bg-hover" onclick={closeInboxMessageModal}>Close</button>
          <button
            class="px-3 py-1.5 rounded bg-accent text-black text-sm hover:opacity-90"
            onclick={() => {
              if (!selectedInboxMessage) return;
              app.cmd("open_inbox_item", { id: selectedInboxMessage.id });
              closeInboxMessageModal();
            }}
          >
            Open target
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Projects -->
  <div class="px-2 pt-2 pb-2">
    <div class="flex items-center px-2 mb-1">
      <span class="text-[10px] uppercase tracking-wider text-muted">Projects</span>
      <button
        type="button"
        onclick={() => app.cmd("refresh_pr_list")}
        disabled={loadingPrList}
        title="Refresh PR list"
        aria-label="Refresh PR list"
        class="ml-auto text-muted hover:text-fg disabled:opacity-40 disabled:cursor-not-allowed"
      >
        <svg
          width="11"
          height="11"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class={loadingPrList ? "animate-spin" : ""}
        >
          <path d="M21 12a9 9 0 1 1-9-9 9 9 0 0 1 7.8 4.5"/>
          <polyline points="21 3 21 8 16 8"/>
        </svg>
      </button>
    </div>
    <div class="space-y-0.5">
      {#if filteredProjects.length > 0}
        {#each filteredProjects as project (project.id)}
          {@const badge = projectBadge(project)}
          {@const open = isProjectOpen(project)}
          <div class="group relative flex items-center">
            <button
              type="button"
              onclick={() => toggleProject(project)}
              class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-sm text-left {project.is_active ? 'text-fg-2' : 'text-fg-3'}"
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7l9-5 9 5v10l-9 5-9-5V7z"/><path d="m3 7 9 5 9-5M12 22V12"/></svg>
              <span class="truncate">{project.name}</span>
              {#if badge > 0}
                <span class="font-mono text-[10px] text-muted ml-auto pr-5">{badge}</span>
              {/if}
            </button>
            <button
              type="button"
              onclick={(e) => openBranchPicker(project.id, e)}
              title="Track another branch"
              aria-label="Track another branch in {project.name}"
              class="absolute right-1 opacity-0 group-hover:opacity-100 px-1 text-muted hover:text-fg"
            >+</button>
            {#if addingTo === project.id}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="fixed inset-0 z-[150]" onclick={closeBranchPicker}></div>
              <div class="absolute right-0 top-full mt-1 z-[151] w-56 max-h-64 overflow-y-auto rounded-md border border-border bg-card shadow-xl py-1">
                {#if pickerLoading}
                  <div class="px-3 py-2 text-xs text-muted">Loading…</div>
                {:else if availableBranches.length === 0}
                  <div class="px-3 py-2 text-xs text-muted">No other local branches</div>
                {:else}
                  {#each availableBranches as name (name)}
                    <button
                      type="button"
                      onclick={() => pickBranch(project.id, name)}
                      class="w-full text-left px-3 py-1.5 text-sm text-fg-2 hover:bg-hover truncate"
                      title={name}
                    >{name}</button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>
          {#if open}
            <div class="ml-4 pl-3 border-l border-hairline space-y-0.5">
              {#if project.pr_cache_stale}
                <div class="flex items-center gap-1.5 px-2 py-1">
                  <span class="text-[9px] uppercase tracking-wider text-amber-400">Stale</span>
                  {#if project.pr_cache_age_ms != null}
                    <span class="text-[9px] text-muted">{formatCacheAge(project.pr_cache_age_ms)} old</span>
                  {/if}
                  {#if loadingPrList}
                    <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/></svg>
                  {/if}
                </div>
              {/if}
              {#if visibleBranches(project).length > 0}
                <div class="text-[9px] uppercase tracking-wider text-muted px-2 py-1">Tracked</div>
                {#each visibleBranches(project) as br (br.name)}
                  {@const isActiveView = activeTab?.branch === br.name && activeTab?.repo_root === project.root_path}
                  {@const branchPending = pendingBranchKey === `${project.id}:${br.name}`}
                  <div class="group relative flex items-center">
                    <button
                      type="button"
                      title={br.name}
                      onclick={(e) => openBranch(project.id, br.name, e)}
                      onauxclick={(e) => { if (e.button === 1) openBranch(project.id, br.name, e); }}
                      class="w-full flex items-center gap-2 px-2 py-1 rounded-md text-sm text-left {(isActiveView || branchPending) ? 'bg-accent/15 text-fg font-medium' : 'text-fg-3 hover:bg-hover'}"
                    >
                      {#if isActiveView}
                        <span class="w-1.5 h-1.5 rounded-full {br.is_merged ? 'bg-purple-400' : 'bg-accent'} shrink-0"></span>
                      {:else}
                        <span class="w-1.5 h-1.5 rounded-full {br.is_merged ? 'bg-purple-400' : 'bg-ink-500'} shrink-0"></span>
                      {/if}
                      <div class="flex flex-col min-w-0 flex-1">
                        <span class="truncate">{br.name}</span>
                        {#if br.worktree_path != null}
                          <span class="truncate text-[10px] text-muted font-mono">{br.worktree_path.split("/").slice(-2).join("/")}</span>
                        {/if}
                      </div>
                      {#if branchPending}
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0 ml-auto">
                          <path d="M21 12a9 9 0 1 1-3-6.7L21 8"/>
                          <path d="M21 3v5h-5"/>
                        </svg>
                      {/if}
                    </button>
                    {#if !br.is_current}
                      <button
                        type="button"
                        onclick={(e) => branchRowAction(project.id, br.name, e)}
                        title="Remove from view"
                        aria-label="Remove branch {br.name} from view"
                        class="absolute right-1 opacity-0 group-hover:opacity-100 px-1 text-muted hover:text-del-fg"
                      >×</button>
                    {/if}
                  </div>
                {/each}
              {/if}

              {#snippet prRow(pr: PrInfo)}
                {@const isActivePr = (activeTab?.kind === "remote_pr" && activeTab.pr_number === pr.number) ||
                  (activeTab?.kind === "local_branch" && activeTab.branch === pr.head_ref && activeTab.repo_root === project.root_path)}
                {@const prPending = pendingPrKey === `${project.id}:${pr.number}`}
                <button
                  type="button"
                  title="{pr.title} #{pr.number}"
                  onclick={(e) => openPr(project.id, pr.number, pr.head_ref, e, pr)}
                  onauxclick={(e) => { if (e.button === 1) openPr(project.id, pr.number, pr.head_ref, e, pr); }}
                  onmouseenter={() => schedulePrPrefetch(project.id, pr)}
                  onmouseleave={() => cancelPrPrefetch(project.id, pr.number)}
                  class="w-full flex items-center gap-2 px-2 py-1 rounded-md text-left {(isActivePr || prPending) ? 'bg-accent/15 text-fg font-medium' : 'hover:bg-hover text-fg-3'}"
                >
                  <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="{prIconColor(pr)} shrink-0">
                    <line x1="6" y1="3" x2="6" y2="15"/>
                    <circle cx="18" cy="6" r="3"/>
                    <circle cx="6" cy="18" r="3"/>
                    <path d="M18 9a9 9 0 0 1-9 9"/>
                  </svg>
                  <span class="truncate text-sm">{pr.title}</span>
                  {#if prPending}
                    <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0">
                      <path d="M21 12a9 9 0 1 1-3-6.7L21 8"/>
                      <path d="M21 3v5h-5"/>
                    </svg>
                  {/if}
                  <span class="shrink-0 text-[10px] mono text-muted">#{pr.number}</span>
                </button>
              {/snippet}

              {#snippet prSectionLabel(text: string)}
                <div class="flex items-center gap-1.5 px-2 py-1 mt-1">
                  <span class="text-[9px] uppercase tracking-wider text-muted">{text}</span>
                  {#if loadingPrList}
                    <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/></svg>
                  {/if}
                </div>
              {/snippet}

              {#if visibleMyPrs(project).length > 0 || (loadingPrList && project.my_prs?.length === 0 && !searchActive)}
                {@render prSectionLabel("My PRs")}
                {#each visibleMyPrs(project) as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
              {/if}

              {#if visibleToReviewPrs(project).length > 0 || (loadingPrList && project.prs_to_review?.length === 0 && !searchActive)}
                {@render prSectionLabel("To Review")}
                {@const toReviewVisible = visibleToReviewPrs(project).slice(0, visibleToReviewCount(project.id))}
                {#each toReviewVisible as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
                {#if visibleToReviewPrs(project).length > toReviewVisible.length}
                  <button
                    type="button"
                    onclick={() => revealMoreToReview(project.id)}
                    class="w-full text-left px-2 py-1 rounded-md text-xs text-fg-3 hover:bg-hover"
                  >
                    Show more
                  </button>
                {/if}
              {/if}

              {#if visibleRecentlyMergedPrs(project).length > 0 || (loadingPrList && project.recently_merged?.length === 0 && !searchActive)}
                {@render prSectionLabel("Recently Merged")}
                {#each visibleRecentlyMergedPrs(project) as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
              {/if}
            </div>
          {/if}
        {/each}
      {:else if projects.length > 0 && sidebarSearchNeedle}
        <div class="px-2 py-2 text-xs text-muted">No matching projects, branches, or PRs.</div>
      {:else}
        <!-- Fallback: legacy single project derived from worktrees -->
        <div class="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-sm text-fg-2">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7l9-5 9 5v10l-9 5-9-5V7z"/><path d="m3 7 9 5 9-5M12 22V12"/></svg>
          <span class="truncate">{fallbackProjectName}</span>
          {#if worktrees.length > 1}
            <span class="font-mono text-[10px] text-muted ml-auto">{worktrees.length}</span>
          {/if}
        </div>
      {/if}
    </div>
  </div>
  </div>

  <!-- Footer: er + Settings — fixed at bottom by being a sibling of the flex-1 scroll area. -->
  <button
    onclick={() => (settingsOpen = true)}
    class="border-t border-hairline p-3 flex items-center gap-2 text-sm text-fg-3 shrink-0 hover:bg-hover text-left"
  >
    <div class="w-6 h-6 rounded-md bg-accent flex items-center justify-center text-black text-xs font-bold">er</div>
    <span>Settings</span>
  </button>
</aside>
{/if}

<svelte:window onkeydown={onSettingsKey} />

{#if settingsOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-[200] bg-black/50" onclick={() => (settingsOpen = false)}></div>
  <div class="fixed left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 z-[201] w-[420px] rounded-xl bg-card border border-border shadow-2xl p-5">
    <div class="text-base font-semibold text-fg mb-4">Settings</div>
    <label class="block text-xs uppercase tracking-wider text-muted mb-1">
      AI model
      <select
        onchange={(e) => app.cmd("set_ai_model", { model: e.currentTarget.value })}
        class="mt-1 w-full bg-surface border border-hairline rounded-md px-2 py-1.5 text-sm text-fg outline-none"
      >
        <option value="opus">Opus</option>
        <option value="sonnet">Sonnet</option>
        <option value="haiku">Haiku</option>
      </select>
    </label>
  </div>
{/if}
