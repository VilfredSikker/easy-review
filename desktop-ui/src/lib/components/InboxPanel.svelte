<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";
  import {
    formatInboxAge,
    formatInboxUpdated,
    groupInboxItems,
    inboxCategoryChips,
    inboxItemProjectId,
    inboxKindMeta,
    sortInboxItems,
    type InboxCategoryId,
  } from "$lib/inboxCategories";

  const snapshot = $derived(app.snapshot);
  const projects = $derived<ProjectSnapshot[]>(snapshot?.projects ?? []);
  const inboxItems = $derived<InboxItemSnapshot[]>(snapshot?.inbox_items ?? []);
  const inboxUnreadCount = $derived<number>(snapshot?.inbox_unread_count ?? 0);
  const inboxLastRefreshMs = $derived<number>(snapshot?.inbox_last_refresh_ms ?? 0);

  let inboxPopoverOpen = $state(false);
  let inboxFilter = $state<"all" | "unread" | "read">("all");
  let inboxProjectFilterChoice = $state<"all" | string>("all");
  let inboxCategoryFilterChoice = $state<"all" | InboxCategoryId>("all");
  let selectedInboxMessage = $state<InboxItemSnapshot | null>(null);

  const inboxVisible = $derived(sortInboxItems(inboxItems).slice(0, 20));
  const inboxTeaser = $derived(sortInboxItems(inboxItems).slice(0, 2));

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

  const inboxProjectOptions = $derived(
    projects
      .filter((p) => inboxItems.some((item) => inboxItemProjectId(item, projects) === p.id))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  const inboxProjectFilter = $derived(
    inboxProjectFilterChoice !== "all" &&
      !inboxProjectOptions.some((p) => p.id === inboxProjectFilterChoice)
      ? "all"
      : inboxProjectFilterChoice,
  );

  const inboxByProject = $derived(
    inboxProjectFilter === "all"
      ? inboxVisible
      : inboxVisible.filter((i) => inboxItemProjectId(i, projects) === inboxProjectFilter),
  );

  const inboxByRead = $derived(
    inboxByProject.filter((i) => {
      if (inboxFilter === "unread") return i.read_at_ms == null;
      if (inboxFilter === "read") return i.read_at_ms != null;
      return true;
    }),
  );

  const categoryChips = $derived(inboxCategoryChips(inboxByRead));

  const inboxCategoryFilter = $derived(
    inboxCategoryFilterChoice !== "all" &&
      !categoryChips.some((c) => c.category === inboxCategoryFilterChoice)
      ? "all"
      : inboxCategoryFilterChoice,
  );

  const inboxFiltered = $derived(
    inboxCategoryFilter === "all"
      ? inboxByRead
      : inboxByRead.filter((i) => (i.category ?? "other") === inboxCategoryFilter),
  );

  const inboxGroups = $derived(groupInboxItems(inboxFiltered));
  const inboxUnreadCountAll = $derived(inboxByProject.filter((i) => i.read_at_ms == null).length);
  const groupedView = $derived(inboxCategoryFilter === "all");
</script>

{#snippet inboxRow(item: InboxItemSnapshot, onClick: () => void)}
  {@const meta = inboxKindMeta(item)}
  {@const isUnread = item.read_at_ms == null}
  <button
    type="button"
    onclick={onClick}
    class="w-full text-left flex items-start gap-[10px] px-[10px] py-2 rounded-md hover:bg-hover relative"
  >
    {#if isUnread}
      <span class="absolute left-0 top-3 bottom-3 w-0.5 bg-accent rounded-r-sm"></span>
    {/if}
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 mt-0.5 {meta.color}">
      <path d={meta.path} />
    </svg>
    <div class="flex-1 min-w-0">
      <div class="text-[12px] {isUnread ? 'font-medium text-fg-2' : 'text-fg-3'} truncate leading-snug">{item.title}</div>
      {#if item.body}
        <div class="text-[11px] text-muted truncate mt-0.5">{item.body}</div>
      {/if}
    </div>
    <span class="text-[10px] text-muted shrink-0 mt-0.5 whitespace-nowrap">{formatInboxAge(item.created_at_ms)}</span>
  </button>
{/snippet}

<div class="px-2 pt-3 pb-1">
  <div class="flex items-center px-2 mb-1.5">
    <button
      type="button"
      onclick={openInboxPopover}
      class="text-[10px] font-semibold uppercase tracking-[0.06em] text-muted hover:text-fg-2 transition-colors"
    >
      Inbox
    </button>
    {#if inboxUnreadCount > 0}
      <span class="ml-1.5 text-[9px] font-mono bg-ink-700 text-accent px-1 rounded-full">{inboxUnreadCount}</span>
    {/if}
    <button
      type="button"
      onclick={() => app.cmd("refresh_notifications")}
      class="ml-auto text-[10px] text-muted hover:text-fg transition-colors"
      title="Refresh notifications"
      aria-label="Refresh notifications"
    >↻</button>
  </div>

  <div class="space-y-0.5">
    {#if inboxTeaser.length === 0}
      <div class="px-2 py-1 text-[12px] text-muted">No notifications</div>
    {:else}
      {#each inboxTeaser as item (item.id)}
        {@const meta = inboxKindMeta(item)}
        {@const isUnread = item.read_at_ms == null}
        <button
          type="button"
          onclick={openInboxPopover}
          class="w-full text-left flex items-start gap-2 px-2 py-1.5 rounded-md hover:bg-hover relative group"
        >
          {#if isUnread}
            <span class="absolute left-0 top-2 bottom-2 w-0.5 bg-accent rounded-r-sm"></span>
          {/if}
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 mt-0.5 {meta.color}">
            <path d={meta.path} />
          </svg>
          <div class="min-w-0 flex-1">
            <div class="text-[12px] {isUnread ? 'font-medium text-fg-2' : 'text-fg-3'} truncate leading-tight">{item.title}</div>
            {#if item.body}
              <div class="text-[11px] text-muted truncate mt-0.5">{item.body}</div>
            {/if}
          </div>
          <span class="text-[10px] text-muted shrink-0 mt-0.5">{formatInboxAge(item.created_at_ms)}</span>
        </button>
      {/each}
      {#if inboxUnreadCount > 2}
        <button
          type="button"
          onclick={openInboxPopover}
          class="w-full text-left flex items-center gap-1.5 px-2 py-1 text-[11px] text-muted hover:text-fg-2 transition-colors"
        >
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M6 9l6 6 6-6"/></svg>
          See {inboxUnreadCount - 2} more
        </button>
      {/if}
    {/if}
  </div>
</div>

{#if inboxPopoverOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-[200]" onclick={closeInboxPopover}></div>
  <div
    class="absolute left-2 top-28 z-[201] w-96 rounded-lg border border-border bg-ink-800 shadow-xl flex flex-col overflow-hidden"
    style="max-height: calc(100vh - 120px);"
  >
    <div class="px-3 pt-2.5 pb-2 border-b border-hairline flex flex-col gap-2">
      <div class="flex items-center gap-1.5">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent shrink-0">
          <path d="M22 12h-6l-2 3h-4l-2-3H2"/>
          <path d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>
        </svg>
        <span class="text-[12px] font-semibold text-fg">Inbox</span>
        <span class="text-[11px] text-muted">· Updated {formatInboxUpdated(inboxLastRefreshMs)}</span>
        <div class="flex-1"></div>
        <button
          type="button"
          onclick={closeInboxPopover}
          title="Close"
          aria-label="Close inbox"
          class="w-5 h-5 rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover"
        >
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
        </button>
      </div>
      <div class="flex items-center gap-2">
        <div class="inline-flex bg-surface border border-hairline rounded p-0.5">
          <button
            type="button"
            onclick={() => (inboxFilter = "all")}
            class="h-[22px] px-2 rounded-sm text-[11px] font-medium flex items-center gap-1 {inboxFilter === 'all' ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
          >All <span class="text-muted ml-0.5">{inboxByProject.length}</span></button>
          <button
            type="button"
            onclick={() => (inboxFilter = "unread")}
            class="h-[22px] px-2 rounded-sm text-[11px] font-medium flex items-center gap-1 {inboxFilter === 'unread' ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
          >Unread <span class="{inboxFilter === 'unread' ? 'text-accent' : 'text-muted'} ml-0.5">{inboxUnreadCountAll}</span></button>
          <button
            type="button"
            onclick={() => (inboxFilter = "read")}
            class="h-[22px] px-2 rounded-sm text-[11px] font-medium {inboxFilter === 'read' ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
          >Read</button>
        </div>
        <div class="flex-1"></div>
        <button
          type="button"
          onclick={() => { app.cmd("mark_all_inbox_read"); }}
          title="Mark all read"
          aria-label="Mark all read"
          class="w-6 h-6 rounded flex items-center justify-center text-periwinkle hover:text-fg hover:bg-hover"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 7 17l-3-3"/><path d="m22 10-7.5 7.5L13 16"/></svg>
        </button>
        <button
          type="button"
          onclick={() => { app.cmd("clear_read_inbox_items"); }}
          title="Clear read"
          aria-label="Clear read"
          class="w-6 h-6 rounded flex items-center justify-center text-periwinkle hover:text-fg hover:bg-hover"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18M19 6l-1 14H6L5 6M10 11v6M14 11v6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
        </button>
      </div>
      {#if categoryChips.length > 0}
        <div class="flex flex-wrap gap-1" role="tablist" aria-label="Inbox categories">
          <button
            type="button"
            onclick={() => (inboxCategoryFilterChoice = "all")}
            class="h-[20px] px-1.5 rounded text-[10px] font-medium border {inboxCategoryFilter === 'all' ? 'bg-hover text-fg border-border' : 'text-muted border-hairline hover:text-fg-2'}"
          >All</button>
          {#each categoryChips as chip (chip.category)}
            <button
              type="button"
              onclick={() => (inboxCategoryFilterChoice = chip.category)}
              class="h-[20px] px-1.5 rounded text-[10px] font-medium border flex items-center gap-1 {inboxCategoryFilter === chip.category ? 'bg-hover text-fg border-border' : 'text-muted border-hairline hover:text-fg-2'}"
            >
              {chip.label}
              {#if chip.unread > 0}
                <span class="{inboxCategoryFilter === chip.category ? 'text-accent' : 'text-muted'}">{chip.unread}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
      {#if inboxProjectOptions.length > 0}
        <div class="flex items-center gap-2">
          <label for="inbox-project-filter" class="text-[11px] text-muted shrink-0">Project</label>
          <select
            id="inbox-project-filter"
            value={inboxProjectFilter}
            onchange={(e) => {
              inboxProjectFilterChoice = e.currentTarget.value;
            }}
            class="flex-1 min-w-0 bg-surface border border-hairline rounded px-2 py-1 text-[11px] text-fg outline-none"
          >
            <option value="all">All</option>
            {#each inboxProjectOptions as project (project.id)}
              <option value={project.id}>{project.name}</option>
            {/each}
          </select>
        </div>
      {/if}
    </div>
    <div class="flex-1 overflow-y-auto p-1">
      {#if inboxFiltered.length === 0}
        <div class="px-3 py-6 text-center text-[12px] text-muted">No items</div>
      {:else if groupedView}
        {#each inboxGroups as group (group.category)}
          <div class="px-2 pt-2 pb-1 text-[10px] font-semibold uppercase tracking-[0.06em] text-muted">
            {group.label}
            <span class="ml-1 font-mono normal-case tracking-normal">{group.items.length}</span>
          </div>
          {#each group.items as item (item.id)}
            {@render inboxRow(item, () => openInboxMessageModal(item))}
          {/each}
        {/each}
      {:else}
        {#each inboxFiltered as item (item.id)}
          {@render inboxRow(item, () => openInboxMessageModal(item))}
        {/each}
      {/if}
    </div>
  </div>
{/if}

{#if selectedInboxMessage}
  <ModalShell
    open={true}
    ariaLabel={selectedInboxMessage.title}
    onClose={closeInboxMessageModal}
    backdropClass="fixed inset-0 z-[250] bg-bg/60"
    panelClass="fixed left-1/2 top-1/2 z-[251] w-full max-w-2xl -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-surface shadow-xl outline-none"
  >
    <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
      <span class={selectedInboxMessage.severity === "error" ? "text-del-fg" : selectedInboxMessage.severity === "warning" ? "text-warning" : "text-muted"}>●</span>
      <div class="text-sm text-fg-1 truncate">{selectedInboxMessage.title}</div>
      <button class="ml-auto text-muted hover:text-fg px-2" onclick={closeInboxMessageModal}>×</button>
    </div>
    <div class="px-4 py-3 text-sm text-fg-2 whitespace-pre-wrap break-words max-h-[50vh] overflow-y-auto">
      {selectedInboxMessage.body || "(No message body)"}
    </div>
    <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2">
      <button class="px-3 py-1.5 rounded border border-border text-sm text-fg-2 hover:bg-hover" onclick={closeInboxMessageModal}>Close</button>
      <button
        class="px-3 py-1.5 rounded bg-accent text-on-accent text-sm hover:opacity-90"
        onclick={() => {
          if (!selectedInboxMessage) return;
          app.cmd("open_inbox_item", { id: selectedInboxMessage.id });
          closeInboxMessageModal();
        }}
      >
        Open target
      </button>
    </div>
  </ModalShell>
{/if}
