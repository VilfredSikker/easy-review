<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { groupInboxThreads, isThreadUnread } from "$lib/inboxThreads";

  // Counted per PR, like the Inbox page's rows, so the badge matches what the
  // reader finds when they click through.
  const unreadPrs = $derived(
    groupInboxThreads(app.snapshot?.inbox_items ?? [], app.snapshot?.projects ?? []).filter(isThreadUnread)
      .length,
  );
  const on = $derived(app.mainView === "inbox");
</script>

<div class="px-2 pt-2">
  <button
    type="button"
    onclick={() => app.setMainView("inbox")}
    aria-current={on ? "page" : undefined}
    title="Inbox"
    class="w-full flex items-center gap-2 px-2 py-[5px] rounded-md text-[12.5px] text-left min-w-0 {on ? 'bg-hover text-fg shadow-[inset_0_0_0_1px_var(--color-hairline)]' : 'text-fg-2 hover:bg-hover hover:text-fg'}"
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0">
      <path d="M22 12h-6l-2 3h-4l-2-3H2"/>
      <path d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>
    </svg>
    <span class="flex-1 min-w-0 truncate">Inbox</span>
    {#if unreadPrs > 0}
      <span class="font-mono text-[10px] leading-4 px-[5px] rounded-lg text-accent bg-accent-soft">{unreadPrs}</span>
    {/if}
  </button>
</div>
