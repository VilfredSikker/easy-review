<script lang="ts">
  import { app } from "$lib/stores/app.svelte";

  const snapshot = $derived(app.snapshot);
  const mode = $derived(snapshot?.mode ?? "branch");
  const reviewed = $derived(snapshot?.reviewed_count ?? 0);
  const total = $derived(snapshot?.total_count ?? 0);
  const progress = $derived(total > 0 ? Math.round((reviewed / total) * 100) : 0);
  const ai = $derived(snapshot?.ai);

  const modes = [
    { key: "branch", label: "B", title: "Branch diff [1]", shortcut: "1" },
    { key: "unstaged", label: "U", title: "Unstaged changes [2]", shortcut: "2" },
    { key: "staged", label: "S", title: "Staged changes [3]", shortcut: "3" },
    { key: "history", label: "H", title: "Commit history [4]", shortcut: "4" },
  ] as const;
</script>

<aside class="w-11 border-r border-ink-500/40 bg-ink-850 shrink-0 flex flex-col items-center pt-2 pb-3 gap-1">
  <!-- Mode buttons -->
  {#each modes as m}
    <button
      class="w-8 h-8 rounded flex items-center justify-center text-[11px] font-mono font-medium transition-colors
        {mode === m.key ? 'bg-accent/20 text-accent' : 'text-ink-400 hover:bg-ink-700 hover:text-ink-200'}"
      onclick={() => app.cmd("set_mode", { mode: m.key })}
      title={m.title}
    >
      {m.label}
    </button>
  {/each}

  <div class="w-6 border-t border-ink-500/40 my-1"></div>

  <!-- Jump to unreviewed -->
  <button
    class="w-8 h-8 rounded flex items-center justify-center text-ink-400 hover:bg-ink-700 hover:text-ink-200 transition-colors"
    onclick={() => app.cmd("jump_to_unreviewed")}
    title="Jump to unreviewed [U]"
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <circle cx="12" cy="12" r="10"/>
      <path d="M12 8v4M12 16h.01"/>
    </svg>
  </button>

  <!-- Refresh -->
  <button
    class="w-8 h-8 rounded flex items-center justify-center text-ink-400 hover:bg-ink-700 hover:text-ink-200 transition-colors"
    onclick={() => app.cmd("refresh_diff")}
    title="Refresh diff [R]"
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>
      <path d="M21 3v5h-5"/>
      <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>
      <path d="M8 16H3v5"/>
    </svg>
  </button>

  <div class="flex-1"></div>

  <!-- Risk indicators -->
  {#if ai && (ai.high > 0 || ai.med > 0 || ai.low > 0)}
    <div class="flex flex-col items-center gap-0.5">
      {#if ai.high > 0}
        <span class="text-[10px] font-mono text-risk-high leading-none">{ai.high}H</span>
      {/if}
      {#if ai.med > 0}
        <span class="text-[10px] font-mono text-risk-med leading-none">{ai.med}M</span>
      {/if}
      {#if ai.low > 0}
        <span class="text-[10px] font-mono text-risk-low leading-none">{ai.low}L</span>
      {/if}
    </div>
    <div class="w-6 border-t border-ink-500/40 my-1"></div>
  {/if}

  <!-- Progress arc -->
  <div class="flex flex-col items-center gap-0.5" title="{reviewed}/{total} files reviewed">
    <svg width="28" height="28" viewBox="0 0 28 28">
      <circle cx="14" cy="14" r="11" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ink-700"/>
      <circle
        cx="14" cy="14" r="11"
        fill="none"
        stroke="currentColor"
        stroke-width="2.5"
        stroke-dasharray="{69.1}"
        stroke-dashoffset="{69.1 - (progress / 100) * 69.1}"
        stroke-linecap="round"
        transform="rotate(-90 14 14)"
        class="text-accent transition-all duration-300"
      />
    </svg>
    <span class="text-[9px] font-mono text-ink-400 leading-none">{progress}%</span>
  </div>
</aside>
