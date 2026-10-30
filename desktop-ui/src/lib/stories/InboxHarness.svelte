<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import InboxView from "$lib/components/InboxView.svelte";
  import { richSnapshot } from "$lib/stories/fixtures";
  import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";

  interface Props {
    inboxItems?: InboxItemSnapshot[];
    projects?: ProjectSnapshot[];
    /** Render the full Inbox page beside the rail rather than the rail alone. */
    page?: boolean;
  }

  const { inboxItems = [], projects = [], page = false }: Props = $props();

  // Seed app.snapshot so both the rail and the page derive from the store.
  $effect(() => {
    app.snapshot = {
      ...richSnapshot,
      projects,
      inbox_items: inboxItems,
      inbox_unread_count: inboxItems.filter((i) => i.read_at_ms == null).length,
      inbox_last_refresh_ms: inboxItems.length > 0 ? Date.now() - 2 * 60 * 1000 : 0,
    };
    app.setMainView(page ? "inbox" : "diff");
  });
</script>

<div class="bg-bg text-fg h-screen flex">
  <!-- Fixed-width rail container: mirrors the real left sidebar slot. -->
  <div class="bg-ink-900 h-full shrink-0" style="width: 240px">
    <LeftSidebar collapsed={false} />
  </div>
  {#if page}
    <div class="flex-1 min-w-0 h-full">
      <InboxView />
    </div>
  {/if}
</div>
