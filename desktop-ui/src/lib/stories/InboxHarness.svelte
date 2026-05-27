<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import LeftSidebar from "$lib/components/LeftSidebar.svelte";
  import { richSnapshot } from "$lib/stories/fixtures";
  import type { InboxItemSnapshot } from "$lib/types";

  interface Props {
    inboxItems?: InboxItemSnapshot[];
    /**
     * When true, the harness performs a DOM click on the inbox "Inbox" header
     * button after mount so the popover opens without modifying LeftSidebar.
     * Used by the PopoverOpen story.
     */
    autoOpenPopover?: boolean;
  }

  const { inboxItems = [], autoOpenPopover = false }: Props = $props();

  // Seed app.snapshot so LeftSidebar derives the inbox state from the store.
  $effect(() => {
    app.snapshot = {
      ...richSnapshot,
      inbox_items: inboxItems,
      inbox_unread_count: inboxItems.filter((i) => i.read_at_ms == null).length,
      inbox_last_refresh_ms: inboxItems.length > 0 ? Date.now() - 2 * 60 * 1000 : 0,
    };
  });

  onMount(async () => {
    if (!autoOpenPopover) return;
    // Wait one tick for the $effect above to commit, then one animation frame
    // for Svelte to flush the DOM so the inbox buttons are rendered.
    await tick();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    // Click the "Inbox" eyebrow button — find it by its visible text content.
    // All three click targets (eyebrow, teaser items, "See N more") call
    // openInboxPopover, but the eyebrow is always rendered even when there are
    // no items, making it the most reliable target.
    const buttons = document.querySelectorAll<HTMLButtonElement>("button");
    const inboxBtn = Array.from(buttons).find((b) => b.textContent?.trim() === "Inbox");
    inboxBtn?.click();
  });
</script>

<!-- Fixed-width dark rail container — mirrors the real left sidebar slot. -->
<div class="bg-ink-900 text-ink-50 h-screen" style="width: 240px">
  <LeftSidebar collapsed={false} />
</div>
