<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import { commandPalette } from "$lib/stores/commandPalette.svelte";
  import { inboxView } from "$lib/stores/inboxView.svelte";
  import { reviewPage } from "$lib/stores/reviewPage.svelte";
  import TabOpenModal from "$lib/components/TabOpenModal.svelte";
  import { INBOX_CATEGORY_LABELS, formatInboxUpdated, inboxItemCategory, inboxKindMeta } from "$lib/inboxCategories";
  import {
    INBOX_CATEGORY_TONE,
    filterInboxThreads,
    findOpenTab,
    groupInboxThreads,
    inboxReadCounts,
    inboxThreadRef,
    inboxThreadRole,
    inboxThreadTitle,
    inboxToneStyle,
    inboxTypeChips,
    isQuotedCategory,
    isThreadUnread,
    selectedInboxThread,
    stepInboxSelection,
    threadLatestCategory,
    withDaySeparators,
    type InboxThread,
  } from "$lib/inboxThreads";
  import { resolvePrOpen, saveAlwaysNewTab, type TabOpenRequest } from "$lib/prOpen";
  import { longAge, shortAge } from "$lib/relativeTime";
  import type { InboxItemSnapshot, ProjectSnapshot, TabSummary } from "$lib/types";

  const projects = $derived<ProjectSnapshot[]>(app.snapshot?.projects ?? []);
  const tabs = $derived<TabSummary[]>(app.snapshot?.tabs ?? []);
  const lastRefreshMs = $derived(app.snapshot?.inbox_last_refresh_ms ?? 0);
  const threads = $derived(groupInboxThreads(app.snapshot?.inbox_items ?? [], projects));

  const projectOptions = $derived(
    projects
      .filter((p) => threads.some((t) => t.projectId === p.id))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );
  // A remembered filter for a project that has since gone quiet falls back to all.
  const projectId = $derived(
    projectOptions.some((p) => p.id === inboxView.projectId) ? inboxView.projectId : "all",
  );
  const inProject = $derived(filterInboxThreads(threads, { read: "all", projectId, category: "all" }));
  const counts = $derived(inboxReadCounts(inProject));
  const chips = $derived(
    inboxTypeChips(filterInboxThreads(inProject, { read: inboxView.read, projectId: "all", category: "all" })),
  );
  const category = $derived(chips.some((c) => c.category === inboxView.category) ? inboxView.category : "all");
  const visible = $derived(filterInboxThreads(inProject, { read: inboxView.read, projectId: "all", category }));
  const rows = $derived(withDaySeparators(visible));
  const selected = $derived(selectedInboxThread(visible, inboxView.selectedKey));

  let pendingOpen = $state<InboxThread | null>(null);

  function markThreadRead(thread: InboxThread) {
    if (thread.unreadIds.length === 0) return;
    void app.cmd("mark_inbox_items_read", { ids: thread.unreadIds });
  }

  function select(thread: InboxThread) {
    inboxView.selectedKey = thread.key;
    markThreadRead(thread);
    // Warm the open while the reader looks at the timeline. Raw invoke: it
    // returns (), which app.cmd would store as the snapshot.
    invoke("prefetch_inbox_item", { id: thread.latest.id }).catch(() => {
      // Background warmup; a failed one only means a cold open.
    });
  }

  async function step(direction: 1 | -1) {
    const next = stepInboxSelection(visible, selected?.key ?? null, direction);
    if (!next) return;
    select(next);
    await tick();
    document.querySelector('[data-inbox-row][aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }

  function leaveInbox() {
    app.setMainView(reviewPage.available("overview") ? "overview" : "diff");
  }

  /** The open failed (no project for the target) when the active tab is not the target. */
  function landedOn(thread: InboxThread): boolean {
    const snap = app.snapshot;
    const active = snap?.tabs.find((t) => t.idx === snap.active_tab);
    if (!active) return false;
    if (thread.prNumber != null) return active.pr_number === thread.prNumber;
    return active.branch === (thread.latest.target.branch ?? null);
  }

  async function openTarget(thread: InboxThread, newTab: boolean) {
    await app.cmd("open_inbox_item", { id: thread.latest.id, newTab });
    if (landedOn(thread)) leaveInbox();
  }

  async function openThread(thread: InboxThread, e: MouseEvent | KeyboardEvent | null) {
    markThreadRead(thread);
    const existing = findOpenTab(thread, tabs, projects);
    if (existing) {
      await app.cmd("select_tab", { idx: existing.idx });
      leaveInbox();
      return;
    }
    const choice = resolvePrOpen(e instanceof MouseEvent ? e : null);
    if (choice === "ask") pendingOpen = thread;
    else await openTarget(thread, choice === "new-tab");
  }

  function chooseOpen(newTab: boolean, alwaysNewTab: boolean) {
    const thread = pendingOpen;
    pendingOpen = null;
    if (alwaysNewTab) saveAlwaysNewTab(true);
    if (thread) void openTarget(thread, newTab);
  }

  function openRequest(thread: InboxThread): TabOpenRequest {
    const cat = threadLatestCategory(thread);
    const body = thread.latest.body.trim();
    return {
      title: inboxThreadTitle(thread, projects),
      ref: inboxThreadRef(thread, projects),
      context: `${INBOX_CATEGORY_LABELS[cat]} · ${longAge(thread.latest.created_at_ms)}`,
      iconPath: inboxKindMeta(thread.latest).path,
      tone: INBOX_CATEGORY_TONE[cat],
      quote: isQuotedCategory(cat) && body ? body : null,
    };
  }

  const activeTabLabel = $derived(tabs.find((t) => t.is_active)?.label ?? null);

  function markAllRead() {
    const n = counts.unread;
    void app.cmd("mark_all_inbox_read");
    if (n > 0) app.showToast("success", `Marked ${n} read`);
  }

  function clearRead() {
    const n = counts.read;
    void app.cmd("clear_read_inbox_items");
    if (n > 0) app.showToast("success", `Cleared ${n} read`);
  }

  // Registered after the global handler (keyboard.ts), which leaves bare keys
  // alone while the Inbox is the main view so j/k move rows, not diff files.
  $effect(() => {
    function onKeydown(e: KeyboardEvent) {
      if (app.mainView !== "inbox" || commandPalette.open) return;
      if (e.metaKey || e.ctrlKey || e.altKey || document.querySelector("[data-modal]")) return;
      const target = e.target as HTMLElement;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || target.isContentEditable) return;
      if (e.key === "j" || e.key === "k") void step(e.key === "j" ? 1 : -1);
      else if (e.key === "u" && selected) markThreadRead(selected);
      else if (e.key === "Enter" && selected) void openThread(selected, null);
      else return;
      e.preventDefault();
    }
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  });

  function rowSnippet(item: InboxItemSnapshot): string {
    const cat = inboxItemCategory(item);
    const quoted = cat === "pr_comment" || cat === "comment_reply" || cat === "mention";
    return quoted && item.body.trim() ? item.body : item.title;
  }

  const ROLE_LABEL = { author: "You opened this", reviewer: "Your review is requested" } as const;
</script>

{#snippet typeIcon(item: InboxItemSnapshot, size: number)}
  {@const tone = INBOX_CATEGORY_TONE[inboxItemCategory(item)]}
  <span class="w-[26px] h-[26px] rounded-[7px] grid place-items-center shrink-0" style={inboxToneStyle(tone)}>
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d={inboxKindMeta(item).path} /></svg>
  </span>
{/snippet}

{#snippet seg(value: "all" | "unread" | "read", label: string, n: number)}
  <button
    type="button"
    aria-pressed={inboxView.read === value}
    onclick={() => (inboxView.read = value)}
    class="h-[22px] px-2 rounded-sm text-[11.5px] font-medium flex items-center gap-1 {inboxView.read === value ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
  >{label} <span class="text-muted">{n}</span></button>
{/snippet}

<div class="h-full min-h-0 grid grid-cols-1 min-[880px]:grid-cols-[420px_minmax(0,1fr)] bg-bg" data-inbox-view>
  <section class="flex flex-col min-h-0 min-[880px]:border-r border-hairline" aria-label="Inbox">
    <header class="flex items-baseline gap-2 px-3.5 pt-3.5 pb-2">
      <h1 class="text-[15px] font-semibold text-fg">Inbox</h1>
      <span class="text-[11.5px] text-muted">updated {formatInboxUpdated(lastRefreshMs)}</span>
      <span class="flex-1"></span>
      <button
        type="button"
        onclick={() => app.cmd("refresh_notifications")}
        title="Refresh notifications"
        aria-label="Refresh notifications"
        class="w-6 h-6 self-center rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover text-[12px]"
      >↻</button>
      <button
        type="button"
        onclick={markAllRead}
        title="Mark all read"
        aria-label="Mark all read"
        class="w-6 h-6 self-center rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 7 17l-3-3"/><path d="m22 10-7.5 7.5L13 16"/></svg>
      </button>
      <button
        type="button"
        onclick={clearRead}
        title="Clear read"
        aria-label="Clear read"
        class="w-6 h-6 self-center rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 4H8l-7 8 7 8h13a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2zM18 9l-6 6M12 9l6 6"/></svg>
      </button>
    </header>

    <div class="flex items-center gap-2 flex-wrap px-3.5 pb-2">
      <div class="inline-flex bg-surface border border-hairline rounded p-0.5" role="group" aria-label="Read state">
        {@render seg("all", "All", counts.all)}
        {@render seg("unread", "Unread", counts.unread)}
        {@render seg("read", "Read", counts.read)}
      </div>
      {#if projectOptions.length > 0}
        <select
          aria-label="Project"
          value={projectId}
          onchange={(e) => (inboxView.projectId = e.currentTarget.value)}
          class="bg-surface border border-hairline rounded px-2 py-1 text-[11.5px] text-fg outline-none"
        >
          <option value="all">All projects</option>
          {#each projectOptions as project (project.id)}
            <option value={project.id}>{project.name}</option>
          {/each}
        </select>
      {/if}
    </div>

    {#if chips.length > 0}
      <div class="flex flex-wrap gap-1 px-3 pb-2.5 border-b border-hairline" role="group" aria-label="Notification types">
        <button
          type="button"
          aria-pressed={category === "all"}
          onclick={() => (inboxView.category = "all")}
          class="px-2 py-0.5 rounded-full text-[11.5px] border {category === 'all' ? 'bg-hover text-fg border-border' : 'text-fg-3 border-hairline hover:text-fg-2'}"
        >All</button>
        {#each chips as chip (chip.category)}
          {@const tone = INBOX_CATEGORY_TONE[chip.category]}
          <button
            type="button"
            aria-pressed={category === chip.category}
            onclick={() => (inboxView.category = chip.category)}
            class="px-2 py-0.5 rounded-full text-[11.5px] border flex items-center gap-1 {category === chip.category ? 'bg-hover text-fg border-border' : 'text-fg-3 border-hairline hover:text-fg-2'}"
          >
            <span class="w-1.5 h-1.5 rounded-full" style="background: var(--color-{tone})"></span>
            {chip.label}
            <span class="text-muted">{chip.count}</span>
          </button>
        {/each}
      </div>
    {:else}
      <div class="border-b border-hairline"></div>
    {/if}

    <div class="flex-1 min-h-0 overflow-y-auto overscroll-contain px-1.5 py-1" role="listbox" aria-label="Pull requests">
      {#each rows as { thread, day } (thread.key)}
        {@const unread = isThreadUnread(thread)}
        {@const isSel = selected?.key === thread.key}
        {#if day}
          <div class="text-[10.5px] font-semibold tracking-[0.06em] uppercase text-muted px-2 pt-2.5 pb-1">{day}</div>
        {/if}
        <button
          type="button"
          role="option"
          data-inbox-row
          aria-selected={isSel}
          onclick={() => select(thread)}
          ondblclick={(e) => void openThread(thread, e)}
          class="relative w-full grid grid-cols-[26px_minmax(0,1fr)_auto] gap-2.5 items-start px-2 py-[9px] rounded-lg text-left {isSel ? 'bg-row-select' : 'hover:bg-hover'}"
        >
          {#if isSel}
            <span class="absolute left-0 top-2.5 bottom-2.5 w-0.5 rounded-sm bg-accent"></span>
          {/if}
          {@render typeIcon(thread.latest, 13)}
          <span class="grid gap-[3px] min-w-0">
            <span class="text-[13px] truncate {unread ? 'text-fg font-semibold' : 'text-fg-2'}">{inboxThreadTitle(thread, projects)}</span>
            <span class="text-[12px] text-muted truncate">{rowSnippet(thread.latest)}</span>
          </span>
          <span class="grid justify-items-end gap-1 text-[11px]">
            <span class="font-mono text-muted">{shortAge(thread.latest.created_at_ms)}</span>
            {#if thread.items.length > 1}
              <span class="font-mono text-[10px] px-1 rounded bg-hover text-fg-3">{thread.items.length}</span>
            {/if}
            {#if unread}
              <i class="w-[7px] h-[7px] rounded-full bg-accent" aria-label="unread"></i>
            {/if}
          </span>
        </button>
      {:else}
        <div class="px-5 py-5 text-[12.5px] text-muted">Inbox zero for this filter.</div>
      {/each}
    </div>

    <footer class="flex gap-3 px-3.5 py-2 border-t border-hairline text-[11px] text-muted">
      <span><kbd class="font-mono text-[10px] px-1 border border-border rounded-sm mr-0.5">j</kbd><kbd class="font-mono text-[10px] px-1 border border-border rounded-sm mr-0.5">k</kbd> move</span>
      <span><kbd class="font-mono text-[10px] px-1 border border-border rounded-sm mr-0.5">u</kbd> mark read</span>
      <span><kbd class="font-mono text-[10px] px-1 border border-border rounded-sm mr-0.5">↵</kbd> open review</span>
    </footer>
  </section>

  <section class="hidden min-[880px]:block overflow-y-auto min-h-0" aria-label="Notification detail">
    {#if selected}
      {@const role = inboxThreadRole(selected, projects)}
      {@const unread = isThreadUnread(selected)}
      <div class="max-w-[720px] mx-auto px-7 pt-7 pb-12 grid gap-3.5">
        <div class="flex gap-2 items-center flex-wrap">
          <span class="font-mono text-[12px] text-muted">{inboxThreadRef(selected, projects)}</span>
          {#if role}
            <span class="h-5 px-2 rounded-full border border-border text-[11px] text-fg-3 flex items-center">{ROLE_LABEL[role]}</span>
          {/if}
          {#if unread}
            <span class="h-5 px-2 rounded-full text-[11px] flex items-center" style={inboxToneStyle("warning")}>unread</span>
          {/if}
        </div>
        <h2 class="m-0 text-[21px] font-semibold tracking-[-0.015em] text-fg text-balance">{inboxThreadTitle(selected, projects)}</h2>
        <div class="flex gap-1.5 flex-wrap">
          <button
            type="button"
            onclick={(e) => void openThread(selected, e)}
            title="Cmd-click opens a new tab without asking"
            class="px-3 py-1.5 rounded-md bg-accent text-on-accent text-[12px] font-medium hover:bg-accent-hover flex items-center gap-1.5"
          >Open review <span class="font-mono text-[10.5px] opacity-60">↵</span></button>
          {#if unread}
            <button
              type="button"
              onclick={() => markThreadRead(selected)}
              class="px-3 py-1.5 rounded-md border border-border text-[12px] text-fg-2 hover:bg-hover flex items-center gap-1.5"
            >Mark read <span class="font-mono text-[10.5px] text-muted">u</span></button>
          {/if}
        </div>

        <ol class="relative grid gap-1 mt-2 list-none p-0 before:content-[''] before:absolute before:left-[12.5px] before:top-3.5 before:bottom-3.5 before:w-px before:bg-border">
          {#each selected.items as item (item.id)}
            {@const cat = inboxItemCategory(item)}
            <li class="relative grid grid-cols-[26px_minmax(0,1fr)] gap-3 py-1.5">
              <span class="w-[26px] h-[26px] rounded-[7px] grid place-items-center bg-bg shadow-[inset_0_0_0_1px_var(--color-border)]" style="color: var(--color-{INBOX_CATEGORY_TONE[cat]})">
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d={inboxKindMeta(item).path} /></svg>
              </span>
              <div class="min-w-0">
                <div class="flex items-center gap-[7px] text-[12.5px] pt-[3px] min-w-0">
                  <b class="font-medium text-fg-2 truncate">{item.title}</b>
                  <span class="text-muted shrink-0">{INBOX_CATEGORY_LABELS[cat].toLowerCase()}</span>
                  <span class="ml-auto font-mono text-[11px] text-muted shrink-0">{longAge(item.created_at_ms)}</span>
                </div>
                {#if isQuotedCategory(cat) && item.body.trim()}
                  <div class="mt-1.5 px-3 py-2 border border-hairline rounded-[9px] bg-card text-[13px] leading-[1.55] text-fg-2 whitespace-pre-wrap break-words">{item.body}</div>
                {/if}
              </div>
            </li>
          {/each}
        </ol>
      </div>
    {:else}
      <div class="h-full grid place-items-center text-[13px] text-muted">Nothing selected</div>
    {/if}
  </section>
</div>

<TabOpenModal
  request={pendingOpen ? openRequest(pendingOpen) : null}
  tabCount={tabs.length}
  currentTabLabel={activeTabLabel}
  onChoose={chooseOpen}
  onClose={() => (pendingOpen = null)}
/>
