<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { arenaStats } from "$lib/arena/projections";
  import ArenaIcons from "$lib/components/arena/ArenaIcons.svelte";
  import ArenaProcessBracket from "$lib/components/arena/ArenaProcessBracket.svelte";
  import ArenaProcessMatrix from "$lib/components/arena/ArenaProcessMatrix.svelte";
  import ArenaProcessFunnel from "$lib/components/arena/ArenaProcessFunnel.svelte";
  import ArenaFinalTruth from "$lib/components/arena/ArenaFinalTruth.svelte";

  export type ArenaLayoutMode = "bracket" | "matrix" | "funnel";

  interface Props {
    open: boolean;
    snapshot: ArenaRunSnapshot;
    layoutMode?: ArenaLayoutMode;
    onClose: () => void;
    onLayoutMode?: (mode: ArenaLayoutMode) => void;
    onNewRun?: () => void;
  }

  let {
    open,
    snapshot,
    layoutMode = $bindable("bracket"),
    onClose,
    onLayoutMode,
    onNewRun,
  }: Props = $props();

  let selectedId = $state<string | null>(null);

  const stats = $derived(arenaStats(snapshot.run.findings));

  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  function setLayout(mode: ArenaLayoutMode) {
    layoutMode = mode;
    onLayoutMode?.(mode);
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[100] flex flex-col bg-[rgba(8,12,20,0.78)] backdrop-blur-[6px]"
    role="dialog"
    aria-modal="true"
    aria-label="AI Review Arena"
  >
    <header
      class="shrink-0 border-b border-[var(--arena-border)] bg-[var(--arena-bg-1)] px-5 py-3"
    >
      <div class="flex items-center gap-3">
        <ArenaIcons name="trophy" class="text-[var(--arena-periwinkle)]" size={18} />
        <div>
          <h1 class="text-[14px] font-semibold text-[var(--arena-fg)]">AI Review Arena</h1>
          <p class="text-[11px] text-[var(--arena-fg-subtle)]">
            {snapshot.run.id}
            {#if snapshot.run.title}
              · {snapshot.run.title}
            {/if}
            · {snapshot.run.reviewers.length} reviewers · 3 rounds
          </p>
        </div>
        <div class="flex-1"></div>

        <div
          class="flex rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-0)] p-0.5"
          role="tablist"
          aria-label="Process layout"
        >
          {#each [
            { id: "bracket", label: "Bracket", icon: "bracket" },
            { id: "matrix", label: "Matrix", icon: "grid" },
            { id: "funnel", label: "Funnel", icon: "funnel" },
          ] as opt}
            <button
              type="button"
              role="tab"
              aria-selected={layoutMode === opt.id}
              class="flex items-center gap-1 rounded px-2 py-1 text-[10px] font-semibold transition-colors {layoutMode === opt.id
                ? 'bg-[var(--arena-bg-3)] text-[var(--arena-orange)]'
                : 'text-[var(--arena-fg-muted)]'}"
              onclick={() => setLayout(opt.id as ArenaLayoutMode)}
            >
              <ArenaIcons name={opt.icon as "bracket"} size={12} />
              {opt.label}
            </button>
          {/each}
        </div>

        {#if onNewRun}
          <button
            type="button"
            class="inline-flex h-[30px] items-center gap-1.5 rounded-md bg-[var(--arena-periwinkle)] px-3 text-[11px] font-semibold text-white"
            onclick={onNewRun}
          >
            <ArenaIcons name="plus" size={11} />
            New run
          </button>
        {/if}

        <button
          type="button"
          class="inline-flex h-[30px] w-[30px] items-center justify-center rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] text-[var(--arena-fg-muted)]"
          title="Close (Esc)"
          onclick={onClose}
        >
          <ArenaIcons name="x" size={12} />
        </button>
      </div>

      <div class="mt-2 flex flex-wrap items-center gap-2 text-[10px]">
        <span class="font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">Stats</span>
        {#each [
          { label: "Proposed", value: stats.proposed },
          { label: "Kept", value: stats.verdicts.kept, color: "var(--arena-ok)" },
          { label: "Escalated", value: stats.verdicts.escalated, color: "var(--arena-err)" },
          { label: "Merged", value: stats.verdicts.merged, color: "var(--arena-periwinkle)" },
          { label: "Dropped", value: stats.verdicts.dropped, strike: true },
        ] as chip}
          <span
            class="inline-flex items-center gap-1.5 rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-2.5 py-1"
          >
            <span class="font-semibold uppercase tracking-wider text-[var(--arena-fg-subtle)]"
              >{chip.label}</span
            >
            <span
              class="mono text-[13px] font-bold {chip.strike
                ? 'line-through text-[var(--arena-fg-subtle)]'
                : ''}"
              style={chip.color && !chip.strike ? `color:${chip.color}` : ""}>{chip.value}</span
            >
          </span>
        {/each}
      </div>
    </header>

    <div class="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[minmax(0,1fr)_420px]">
      <section class="flex min-h-0 flex-col bg-[var(--arena-bg-0)]">
        <p
          class="shrink-0 border-b border-[var(--arena-border)] px-4 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]"
        >
          The process
        </p>
        {#if layoutMode === "bracket"}
          <ArenaProcessBracket {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
        {:else if layoutMode === "matrix"}
          <ArenaProcessMatrix {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
        {:else}
          <ArenaProcessFunnel {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
        {/if}
      </section>
      <ArenaFinalTruth {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
    </div>
  </div>
{/if}
