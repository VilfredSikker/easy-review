<script lang="ts">
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    mode: "branch" | "unstaged" | "staged" | "history";
    total_count: number;
    reviewed_count: number;
  }

  const { mode, total_count, reviewed_count }: Props = $props();
</script>

<div class="border-t border-ink-500/40 bg-ink-900 shrink-0">
  <button
    class="w-full px-3 py-1.5 text-sm flex items-center gap-2 hover:bg-ink-800 {mode === 'branch' ? 'bg-ink-700 text-ink-100' : 'text-ink-300'}"
    onclick={() => app.cmd("set_mode", { mode: "branch" })}
  >
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12l9-9 9 9M3 16l9-9 9 9M3 20l9-9 9 9"/></svg>
    <span>All changes</span>
    <span class="ml-auto font-mono text-[11px] text-ink-400">{total_count}</span>
  </button>

  <div class="grid grid-cols-2">
    <button
      class="px-3 py-1.5 text-xs flex items-center gap-1.5 hover:bg-ink-800 {mode === 'unstaged' ? 'bg-ink-600 text-ink-100' : 'text-ink-300'}"
      onclick={() => app.cmd("set_mode", { mode: "unstaged" })}
    >
      <span class="w-1.5 h-1.5 rounded-full bg-amber-400 shrink-0"></span>
      Unstaged
    </button>
    <button
      class="px-3 py-1.5 text-xs flex items-center gap-1.5 hover:bg-ink-800 {mode === 'staged' ? 'bg-ink-600 text-ink-100' : 'text-ink-300'}"
      onclick={() => app.cmd("set_mode", { mode: "staged" })}
    >
      <span class="w-1.5 h-1.5 rounded-full bg-add-fg shrink-0"></span>
      Staged
    </button>
  </div>

  <div class="border-t border-ink-500/40 px-3 py-1.5 flex items-center justify-between text-[11px] text-ink-300 font-mono">
    <span>{reviewed_count} / {total_count} reviewed</span>
    <span class="text-ink-500">j/k · U next</span>
  </div>
</div>
