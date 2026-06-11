<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { agentCatalogEntry } from "$lib/arena/agents";
  import { arenaStats } from "$lib/arena/projections";
  import ArenaIcons from "$lib/components/arena/ArenaIcons.svelte";
  import ArenaProcessBracket from "$lib/components/arena/ArenaProcessBracket.svelte";
  import ArenaProcessMatrix from "$lib/components/arena/ArenaProcessMatrix.svelte";
  import ArenaProcessFunnel from "$lib/components/arena/ArenaProcessFunnel.svelte";
  import ArenaFinalTruth from "$lib/components/arena/ArenaFinalTruth.svelte";
  import ArenaFindingDetail from "$lib/components/arena/ArenaFindingDetail.svelte";
  import ArenaVoteLegend from "$lib/components/arena/ArenaVoteLegend.svelte";
  import { startWindowDrag } from "$lib/windowDrag";

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
  const isSingleReview = $derived(snapshot.run.reviewers.length === 1);
  const agentLabel = $derived.by(() => {
    const kind = snapshot.run.config.agent_kind;
    if (!kind) return null;
    return agentCatalogEntry(kind, kind, "").label;
  });
  const totalRounds = $derived(snapshot.run.config.rounds ?? 1);
  const emptyDiff = $derived((snapshot.run.cost_estimate?.tokens_in ?? 0) === 0);
  const runFailed = $derived(snapshot.run.status === "failed");
  const failedReviewers = $derived(
    snapshot.run.reviewers.flatMap((r) => {
      if (typeof r.status === "object" && r.status !== null && "failed" in r.status) {
        return [`${r.name}: ${r.status.failed.reason}`];
      }
      return [];
    }),
  );
  const showEmptyBanner = $derived(
    snapshot.run.findings.length === 0 && (emptyDiff || runFailed),
  );

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
    class="fixed inset-0 z-[100] flex flex-col bg-[color-mix(in_srgb,var(--arena-bg-app)_78%,transparent)] backdrop-blur-[6px]"
    role="dialog"
    aria-modal="true"
    aria-label="AI Review Arena"
  >
    <header
      class="titlebar-drag shrink-0 border-b border-[var(--arena-border)] bg-[var(--arena-bg-1)] py-3 pr-5 pl-5"
      style="padding-left: max(1.25rem, env(titlebar-area-x, 80px))"
      data-tauri-drag-region
      onmousedown={startWindowDrag}
    >
      <div class="flex items-center gap-3 min-w-0">
        <ArenaIcons name="trophy" class="text-[var(--arena-periwinkle)]" size={18} />
        <div class="min-w-0">
          <h1 class="truncate text-[14px] font-semibold text-[var(--arena-fg)]">
            {agentLabel ?? (isSingleReview ? "AI Review" : "AI Review Arena")}
          </h1>
          <p class="text-[11px] text-[var(--arena-fg-subtle)]">
            {snapshot.run.id}
            {#if snapshot.run.title}
              · {snapshot.run.title}
            {:else if agentLabel}
              · {agentLabel}
            {/if}
            · {snapshot.run.reviewers.length} reviewer{snapshot.run.reviewers.length === 1
              ? ""
              : "s"}
            · {totalRounds} round{totalRounds === 1 ? "" : "s"}
            {#if !isSingleReview && totalRounds >= 2}
              · arbiter {snapshot.run.config.arbiter.model_id}
            {/if}
          </p>
        </div>
        <div class="flex-1"></div>

        {#if !isSingleReview}
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
        {/if}

        {#if onNewRun}
          <button
            type="button"
            class="inline-flex h-[30px] items-center gap-1.5 rounded-md bg-[var(--arena-periwinkle)] px-3 text-[11px] font-semibold text-[var(--arena-bg-0)]"
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

    <div
      class="grid min-h-0 flex-1 grid-cols-1 {isSingleReview
        ? selectedId
          ? 'lg:grid-cols-[minmax(0,1fr)_320px]'
          : ''
        : selectedId
          ? 'lg:grid-cols-[minmax(0,1fr)_420px_320px]'
          : 'lg:grid-cols-[minmax(0,1fr)_420px]'}"
    >
      {#if !isSingleReview}
        <section class="flex min-h-0 flex-col bg-[var(--arena-bg-0)]">
          <p
            class="shrink-0 border-b border-[var(--arena-border)] px-4 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]"
          >
            The process
          </p>
          {#if showEmptyBanner}
          <div
            class="mx-3 mt-3 shrink-0 rounded-lg border border-[var(--arena-warn)]/40 bg-[color-mix(in_srgb,var(--arena-warn)_8%,transparent)] px-4 py-3 text-[12px] text-[var(--arena-fg-muted)]"
          >
            {#if emptyDiff}
              <p class="font-medium text-[var(--arena-fg)]">No diff was captured for this run</p>
              <p class="mt-1 text-[11px] leading-relaxed">
                Scope was <span class="mono">{snapshot.run.config.scope}</span> with base
                <span class="mono">{snapshot.run.base_branch}</span> — there were no changes vs HEAD.
                Start a new run from <strong>Unstaged</strong>, <strong>Staged</strong>, or selected files.
              </p>
            {:else if runFailed}
              <p class="font-medium text-[var(--arena-err)]">Arena run failed</p>
              {#if failedReviewers.length > 0}
                <ul class="mt-2 space-y-1 text-[11px]">
                  {#each failedReviewers as line}
                    <li>{line}</li>
                  {/each}
                </ul>
              {:else}
                <p class="mt-1 text-[11px]">Check terminal logs for <span class="mono">[er-arena]</span>.</p>
              {/if}
            {:else}
              <p class="font-medium text-[var(--arena-fg)]">Reviewers reported no issues</p>
              <p class="mt-1 text-[11px]">The diff had content but no findings were proposed.</p>
            {/if}
          </div>
        {/if}
          {#if layoutMode === "matrix" || layoutMode === "funnel"}
            <div class="shrink-0 border-b border-[var(--arena-border)] px-4 py-2">
              <ArenaVoteLegend compact />
            </div>
          {/if}
          {#if layoutMode === "bracket"}
            <ArenaProcessBracket {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
          {:else if layoutMode === "matrix"}
            <ArenaProcessMatrix {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
          {:else}
            <ArenaProcessFunnel {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
          {/if}
        </section>
      {/if}
      {#if isSingleReview && showEmptyBanner}
        <div
          class="mx-3 mt-3 shrink-0 rounded-lg border border-[var(--arena-warn)]/40 bg-[color-mix(in_srgb,var(--arena-warn)_8%,transparent)] px-4 py-3 text-[12px] text-[var(--arena-fg-muted)] lg:col-span-2"
        >
          {#if emptyDiff}
            <p class="font-medium text-[var(--arena-fg)]">No diff was captured for this run</p>
          {:else if runFailed}
            <p class="font-medium text-[var(--arena-err)]">Review run failed</p>
          {:else}
            <p class="font-medium text-[var(--arena-fg)]">No issues reported</p>
          {/if}
        </div>
      {/if}
      <ArenaFinalTruth {snapshot} {selectedId} onSelect={(id) => (selectedId = id)} />
      {#if selectedId}
        <ArenaFindingDetail
          {snapshot}
          findingId={selectedId}
          onClose={() => (selectedId = null)}
        />
      {/if}
    </div>
  </div>
{/if}
