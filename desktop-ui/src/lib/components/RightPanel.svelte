<script lang="ts">
  import type { AiSnapshot } from "$lib/types";

  interface Props {
    ai: AiSnapshot | null;
  }

  const { ai }: Props = $props();
</script>

<aside class="w-80 border-l border-ink-500/40 bg-ink-850 shrink-0 flex flex-col p-4 gap-3 overflow-y-auto">
  {#if ai}
    <div class="text-xs text-ink-300 uppercase tracking-wider">AI Review</div>
    <div class="text-sm flex gap-2">
      <span class="text-risk-high">{ai.high}H</span>
      <span class="text-ink-400">·</span>
      <span class="text-risk-med">{ai.med}M</span>
      <span class="text-ink-400">·</span>
      <span class="text-risk-low">{ai.low}L</span>
    </div>
    {#if ai.summary_markdown}
      <div class="text-xs text-ink-200 whitespace-pre-wrap overflow-y-auto flex-1">
        {ai.summary_markdown}
      </div>
    {/if}
  {:else}
    <div class="text-xs text-ink-400">No AI review yet</div>
  {/if}
</aside>
