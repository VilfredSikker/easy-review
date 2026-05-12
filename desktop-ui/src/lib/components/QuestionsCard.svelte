<script lang="ts">
  import type { AiSnapshot } from "$lib/types";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  const threads = $derived(ai.threads.filter((t) => t.kind === "question"));

  function basename(path: string): string {
    return path.split("/").pop() ?? path;
  }

  function preview(body: string): string {
    return body.length > 60 ? body.slice(0, 60) + "…" : body;
  }
</script>

<div class="px-3 py-2.5 border-b border-ink-500/40">
  <div class="flex items-center justify-between mb-2">
    <div class="flex items-center gap-1.5">
      <span class="text-[10px] font-medium uppercase tracking-wider text-ink-400">Questions</span>
      <span class="text-[10px] font-mono bg-ink-700 text-ink-300 px-1 py-0.5 rounded">
        {ai.questions}
      </span>
    </div>
  </div>

  {#if threads.length > 0}
    <div class="flex flex-col gap-1">
      {#each threads as thread (thread.id)}
        <div class="flex flex-col gap-0.5 py-1 px-1 rounded {thread.stale ? 'opacity-50' : ''} hover:bg-ink-800/50 transition-colors">
          <div class="flex items-center gap-1.5">
            <span class="text-[10px] text-ink-500 truncate">{basename(thread.file)}</span>
            {#if thread.line}
              <span class="text-[10px] text-ink-600 shrink-0">:{thread.line}</span>
            {/if}
            <span class="flex-1"></span>
            {#if thread.stale}
              <span class="text-[10px] text-amber-500 shrink-0">stale</span>
            {/if}
          </div>
          <div class="flex items-baseline gap-1">
            <span class="text-[10px] font-medium text-question shrink-0">{thread.root.author}</span>
            <span class="text-[11px] text-ink-400 truncate">{preview(thread.root.body_markdown)}</span>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="text-[11px] text-ink-500">No questions</div>
  {/if}
</div>
