<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { ThreadSnapshot } from "$lib/types";

  interface Props {
    thread: ThreadSnapshot;
    hunk_idx: number;
  }

  const { thread }: Props = $props();

  const borderColor = $derived(
    thread.kind === "comment" ? "border-comment" : "border-question"
  );
  const kindColor = $derived(thread.kind === "comment" ? "text-comment" : "text-question");

  let hovered = $state(false);

  function formatTimestamp(ts: string): string {
    try {
      return new Date(ts).toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return ts;
    }
  }

  async function deleteThread() {
    await app.cmd("delete_thread", { id: thread.id });
  }
</script>

<div
  class="border-l-2 {borderColor} bg-ink-900 text-[12px] leading-[1.4] font-mono"
  role="region"
  aria-label="Thread"
  onmouseenter={() => (hovered = true)}
  onmouseleave={() => (hovered = false)}
>
  <!-- Header row -->
  <div class="flex items-center gap-2 px-3 py-1 border-b border-ink-700/60">
    <span class="uppercase text-[10px] font-semibold tracking-wide {kindColor}">
      {thread.kind}
    </span>
    <span class="text-ink-100">{thread.root.author}</span>
    <span class="text-ink-400 text-[11px]">{formatTimestamp(thread.root.timestamp)}</span>
    {#if thread.stale}
      <span class="text-[10px] px-1 rounded bg-ink-700 text-ink-300">stale</span>
    {/if}
    {#if thread.synced}
      <span class="text-add-fg text-[10px]" title="Synced">✓</span>
    {/if}
    <button
      class="ml-auto text-ink-500 hover:text-ink-100 transition-opacity {hovered ? 'opacity-100' : 'opacity-0'}"
      onclick={deleteThread}
      title="Delete thread"
      aria-label="Delete thread"
    >
      ×
    </button>
  </div>

  <!-- Root message body -->
  <div class="px-3 py-1.5 text-ink-200 whitespace-pre-wrap">{thread.root.body_markdown}</div>

  <!-- Replies -->
  {#each thread.replies as reply}
    <div class="ml-4 border-l border-ink-700/60">
      <div class="flex items-center gap-2 px-3 py-0.5">
        <span class="text-ink-300">{reply.author}</span>
        <span class="text-ink-500 text-[11px]">{formatTimestamp(reply.timestamp)}</span>
      </div>
      <div class="px-3 pb-1.5 text-ink-300 whitespace-pre-wrap">{reply.body_markdown}</div>
    </div>
  {/each}
</div>
