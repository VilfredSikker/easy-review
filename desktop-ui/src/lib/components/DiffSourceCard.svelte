<script lang="ts">
  import type { DiffSourceSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    source: DiffSourceSnapshot;
  }

  const { source }: Props = $props();

  let switching = $state<"pr" | "origin" | "local" | null>(null);

  async function switchTo(s: "pr" | "origin" | "local") {
    if (switching) return;
    switching = s;
    try {
      await app.cmd("set_diff_source", { source: s });
    } finally {
      switching = null;
    }
  }

  const labels: Record<string, string> = {
    pr: "PR diff",
    origin: "Origin branch",
    local: "Local branch",
  };

  const badgeClass: Record<string, string> = {
    pr: "bg-purple-500/20 text-purple-300 border border-purple-500/30",
    origin: "bg-blue-500/20 text-blue-300 border border-blue-500/30",
    local: "bg-ink-400/20 text-fg-2 border border-hairline",
  };
</script>

<div class="rounded-md border border-hairline bg-surface p-3 space-y-2.5 text-sm">
  <div class="flex items-center justify-between">
    <span class="text-fg-3 text-xs font-medium uppercase tracking-wide">Diff source</span>
    <span class="px-1.5 py-0.5 rounded text-xs font-medium {badgeClass[source.active] ?? ''}">
      {labels[source.active] ?? source.active}
    </span>
  </div>

  {#if source.status}
    <p class="text-fg-3 text-xs leading-relaxed">{source.status}</p>
  {/if}

  {#if source.suggestion}
    <p class="text-fg-2 text-xs leading-relaxed italic">{source.suggestion}</p>
  {/if}

  {#if source.available.length > 1}
    <div class="flex gap-1.5 flex-wrap">
      {#each source.available as s (s)}
        {@const isActive = s === source.active}
        {@const isLoading = switching === s}
        <button
          disabled={isActive || !!switching}
          onclick={() => switchTo(s)}
          class="px-2 py-1 rounded text-xs font-medium transition-colors
            {isActive
              ? 'bg-accent/20 text-accent border border-accent/40 cursor-default'
              : 'bg-hover text-fg-2 border border-hairline hover:bg-panel hover:text-fg-1 disabled:opacity-40 disabled:cursor-not-allowed'}"
          aria-pressed={isActive}
        >
          {#if isLoading}
            <span class="inline-block w-3 h-3 border border-current border-t-transparent rounded-full animate-spin mr-1"></span>
          {/if}
          {labels[s] ?? s}
        </button>
      {/each}
    </div>
  {/if}
</div>
