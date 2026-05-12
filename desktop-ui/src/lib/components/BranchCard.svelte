<script lang="ts">
  import type { PrSnapshot } from "$lib/types";

  interface Props {
    branch: string;
    base: string;
    pr: PrSnapshot | null;
    reviewed_count: number;
    total_count: number;
  }

  const { branch, base, pr, reviewed_count, total_count }: Props = $props();

  const progress = $derived(total_count > 0 ? (reviewed_count / total_count) * 100 : 0);
  const fraction = $derived(`${reviewed_count}/${total_count}`);
</script>

<div class="px-3 py-2.5 border-b border-ink-500/40">
  <div class="flex items-center justify-between mb-1.5">
    <span class="text-[10px] font-medium uppercase tracking-wider text-ink-400">Branch</span>
    <span class="text-[10px] font-mono text-ink-400">{fraction}</span>
  </div>

  <div class="flex items-baseline gap-1.5 mb-1">
    <span class="text-xs font-mono text-ink-100 truncate">{branch}</span>
    {#if base}
      <span class="text-[10px] text-ink-500 shrink-0">← {base}</span>
    {/if}
  </div>

  {#if pr}
    <div class="flex items-center gap-1.5 mb-1.5">
      <span class="text-[10px] font-mono bg-accent/15 text-accent px-1 py-0.5 rounded shrink-0">
        #{pr.number}
      </span>
      <span class="text-[10px] text-ink-300 truncate" title={pr.title}>{pr.title}</span>
    </div>
  {/if}

  <div class="h-0.5 w-full bg-ink-700 rounded-full overflow-hidden">
    <div
      class="h-full bg-accent rounded-full transition-all duration-300"
      style="width: {progress}%"
    ></div>
  </div>
</div>
