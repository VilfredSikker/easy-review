<script lang="ts">
  import type { ArenaStartConfig, LiveRunEntry, LiveRunState } from "$lib/stores/arena.svelte";
  import { arena, isArenaRunActive } from "$lib/stores/arena.svelte";
  import { agentCatalogEntry } from "$lib/arena/agents";
  import type { ArenaProgressState, ArenaRunSnapshot, Reviewer } from "$lib/types/arena";
  import ArenaIcons from "$lib/components/arena/ArenaIcons.svelte";

  interface Props {
    open: boolean;
    minimized: boolean;
    config: ArenaStartConfig | null;
    liveRuns?: LiveRunEntry[];
    liveRunStates?: Record<string, LiveRunState>;
    snapshot: ArenaRunSnapshot | null;
    progress: ArenaProgressState | null;
    startedAt: number | null;
    onMinimize: () => void;
    onRestore: () => void;
    onCancel: () => void;
    onComplete: () => void;
  }

  const {
    open,
    minimized,
    config,
    liveRuns = [],
    liveRunStates = {},
    snapshot,
    progress,
    startedAt,
    onMinimize,
    onRestore,
    onCancel,
    onComplete,
  }: Props = $props();

  const batchMode = $derived(liveRuns.length > 1);

  let elapsedSec = $state(0);
  let tickTimer: ReturnType<typeof setInterval> | null = null;

  const isArena = $derived(
    (config?.reviewers?.length ?? 0) >= 2 ||
      (config?.agent_groups?.some((g) => g.models.length >= 2) ?? false),
  );
  const totalRounds = $derived(isArena ? (config?.rounds ?? 3) : 1);

  const runProgress = $derived.by(() => {
    if (progress?.phase === "arbiter") {
      return {
        round: totalRounds,
        label: "Arbiter",
        subtitle: "Final verdicts and rationale",
      };
    }
    const st = snapshot?.run.status;
    if (!st) return { round: 1, label: "Starting…", subtitle: "Preparing reviewers" };
    if (st === "queued") return { round: 0, label: "Queued", subtitle: "Waiting to start" };
    if (st === "complete") return { round: totalRounds, label: "Complete", subtitle: "Done" };
    if (st === "cancelled") return { round: 0, label: "Cancelled", subtitle: "Run cancelled" };
    if (st === "failed") {
      const failed = (snapshot?.run.reviewers ?? []).flatMap((r) => {
        if (typeof r.status === "object" && r.status !== null && "failed" in r.status) {
          return [`${r.name}: ${r.status.failed.reason}`];
        }
        return [];
      });
      const subtitle =
        failed.length > 0 ? failed.slice(0, 2).join(" · ") : "All reviewers failed in round 1";
      return { round: 0, label: "Failed", subtitle };
    }
    if (typeof st === "object" && "running" in st) {
      const r = st.running.round;
      const subtitle =
        r === 1
          ? "Each reviewer proposing independently"
          : "Cross-checking findings";
      return { round: r, label: `Round ${r} of ${totalRounds}`, subtitle };
    }
    return { round: 1, label: "Running", subtitle: "Review in progress" };
  });

  const reviewers = $derived.by((): Reviewer[] => {
    const fromSnap = snapshot?.run.reviewers ?? [];
    if (fromSnap.length > 0) return fromSnap;
    return (config?.reviewers ?? []).map((r, i) => ({
      id: `${r.provider_id}-${r.model_id}`,
      name: r.model_id,
      kind: "model" as const,
      provider_id: r.provider_id,
      model_id: r.model_id,
      system_prompt: "",
      color: ["#ff7a2b", "#ff6b6b", "#7f87ff", "#4ec9a4", "#ffc457", "#5fd970"][i % 6],
      icon: "cube",
      tagline: r.provider_id,
      cost_per_1k_in: 0,
      cost_per_1k_out: 0,
      avg_latency_ms: 12_000,
      status: "ok" as const,
    }));
  });

  function reviewerState(r: Reviewer): "thinking" | "done" | "queued" | "failed" {
    if (typeof r.status === "object" && r.status !== null && "failed" in r.status) return "failed";
    if (progress?.done.includes(r.id)) return "done";
    if (progress?.thinking.includes(r.id)) return "thinking";
    return "queued";
  }

  const pct = $derived.by(() => {
    if (progress && reviewers.length > 0 && totalRounds > 0) {
      const doneInRound = reviewers.filter((r) => progress.done.includes(r.id)).length;
      const roundSlice = 100 / totalRounds;
      const base = Math.max(0, progress.round - 1) * roundSlice;
      const inRound = (doneInRound / reviewers.length) * roundSlice;
      return Math.min(100, Math.round(base + inRound));
    }
    const r = runProgress.round;
    return totalRounds > 0 ? Math.min(100, Math.round((r / totalRounds) * 100)) : 0;
  });

  const headline = $derived(
    batchMode
      ? `${liveRuns.length} reviews in progress`
      : isArena
        ? `Arena in progress · Round ${Math.min(runProgress.round || 1, totalRounds)} of ${totalRounds}`
        : "Review in progress",
  );

  function runLabel(entry: LiveRunEntry): string {
    if (entry.title) return entry.title;
    if (entry.agentKind) {
      const meta = agentCatalogEntry(entry.agentKind, entry.agentKind, "");
      return meta.label;
    }
    return entry.runId.slice(0, 12);
  }

  function batchRunStatus(entry: LiveRunEntry): string {
    const st = liveRunStates[entry.runId]?.snapshot?.run.status;
    if (!st) return "Starting…";
    if (st === "complete") return "Complete";
    if (st === "failed") return "Failed";
    if (st === "cancelled") return "Cancelled";
    if (st === "queued") return "Queued";
    if (typeof st === "object" && "running" in st) return `Round ${st.running.round}`;
    return "Running";
  }

  const subhead = $derived(
    isArena
      ? runProgress.subtitle
      : `${reviewers.find((r) => reviewerState(r) === "thinking")?.name ?? reviewers[0]?.name ?? "Reviewer"} is reading the diff`,
  );

  $effect(() => {
    if (snapshot?.run.status === "complete") {
      onComplete();
    }
  });

  $effect(() => {
    if (!open || !startedAt) {
      if (tickTimer) clearInterval(tickTimer);
      tickTimer = null;
      elapsedSec = 0;
      return;
    }
    const update = () => {
      elapsedSec = Math.max(0, (Date.now() - startedAt) / 1000);
    };
    update();
    tickTimer = setInterval(update, 200);
    return () => {
      if (tickTimer) clearInterval(tickTimer);
      tickTimer = null;
    };
  });

  function handleSkip() {
    void arena.skipToResults();
  }
</script>

<style>
  @keyframes arena-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes arena-pulse-dot {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.25;
    }
  }
  .arena-spin-ring {
    animation: arena-spin 1.1s linear infinite;
  }
  .arena-pulse-dot {
    animation: arena-pulse-dot 1.2s ease-in-out infinite;
  }
</style>

{#if open && config}
  {#if minimized}
    <div
      role="button"
      tabindex="0"
      onclick={onRestore}
      onkeydown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onRestore();
        }
      }}
      class="fixed bottom-20 right-6 z-[260] flex max-w-[320px] min-w-[240px] cursor-pointer items-center gap-2.5 rounded-full border border-[var(--arena-border)] bg-[var(--arena-bg-1)] px-3 py-2 shadow-[0_10px_28px_rgba(0,0,0,0.45),0_0_0_1px_rgba(127,135,255,0.18)] hover:border-[var(--arena-periwinkle)]"
      title="Open progress"
      aria-label="Open arena progress"
    >
      <span class="relative flex h-[34px] w-[34px] shrink-0 items-center justify-center">
        <svg class="absolute inset-0 -rotate-90" width="34" height="34" viewBox="0 0 34 34" aria-hidden="true">
          <circle cx="17" cy="17" r="14" fill="none" stroke="var(--arena-bg-3)" stroke-width="2.5" />
          <circle
            cx="17"
            cy="17"
            r="14"
            fill="none"
            stroke="var(--arena-periwinkle)"
            stroke-width="2.5"
            stroke-linecap="round"
            stroke-dasharray="87.96"
            stroke-dashoffset={87.96 * (1 - pct / 100)}
          />
        </svg>
        <span
          class="flex h-5 w-5 items-center justify-center rounded-md text-[#0e1420]"
          style="background: {reviewers.find((r) => reviewerState(r) === 'thinking')?.color ?? 'var(--arena-periwinkle)'}"
        >
          <ArenaIcons name="trophy" size={10} class="text-[#0e1420]" />
        </span>
      </span>
      <span class="min-w-0 flex-1 text-left">
        <span class="block truncate text-[11px] font-medium text-[var(--arena-fg)]">
          {isArena ? runProgress.label : "Review running"}
        </span>
        <span class="block truncate text-[10px] text-[var(--arena-fg-subtle)]">
          {elapsedSec.toFixed(1)}s · {pct}%
        </span>
      </span>
      <button
        type="button"
        class="shrink-0 rounded-full p-1 text-[var(--arena-fg-muted)] hover:bg-[var(--arena-bg-2)] hover:text-[var(--arena-fg)]"
        aria-label="Cancel run"
        onclick={(e) => {
          e.stopPropagation();
          onCancel();
        }}
      >✕</button>
    </div>
  {:else}
    <div
      class="fixed inset-0 z-[260] flex items-center justify-center bg-[rgba(8,12,20,0.82)] backdrop-blur-[6px]"
      role="dialog"
      aria-modal="true"
      aria-label="Review in progress"
    >
      <div
        class="flex w-full max-w-[520px] flex-col gap-[18px] rounded-[14px] border border-[var(--arena-border)] bg-[var(--arena-bg-1)] p-6 shadow-[0_24px_64px_rgba(0,0,0,0.5),0_0_0_1px_rgba(127,135,255,0.2)]"
      >
        <div class="flex items-center gap-2.5">
          <span class="relative inline-flex h-9 w-9 shrink-0 items-center justify-center">
            <span
              class="arena-spin-ring absolute inset-0 rounded-full border-2 border-[rgba(127,135,255,0.2)] border-t-[var(--arena-periwinkle)]"
              aria-hidden="true"
            ></span>
            <span
              class="relative inline-flex h-7 w-7 items-center justify-center rounded-full bg-[rgba(127,135,255,0.16)]"
            >
              <ArenaIcons name="trophy" size={14} class="text-[var(--arena-periwinkle)]" />
            </span>
          </span>
          <div class="min-w-0 flex-1">
            <h2 class="truncate text-[14px] font-semibold text-[var(--arena-fg)]">{headline}</h2>
            <p class="truncate text-[11px] text-[var(--arena-fg-muted)]">{subhead}</p>
          </div>
          <button
            type="button"
            onclick={onMinimize}
            class="inline-flex shrink-0 items-center gap-1 rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] px-2.5 py-1 text-[11px] text-[var(--arena-fg-muted)] hover:border-[var(--arena-border)] hover:text-[var(--arena-fg)]"
          >
            ↘ Run in background
          </button>
        </div>

        {#if batchMode}
          <div class="flex flex-col gap-1.5">
            {#each liveRuns as entry (entry.runId)}
              {@const st = liveRunStates[entry.runId]?.snapshot?.run.status}
              {@const active = st != null && isArenaRunActive(st)}
              <div
                class="flex items-center gap-2 rounded-md border px-2.5 py-2
                  {active
                  ? 'border-[var(--arena-periwinkle)] bg-[rgba(127,135,255,0.1)]'
                  : 'border-[var(--arena-border)] bg-[var(--arena-bg-0)]'}"
              >
                <span class="text-[12px]" aria-hidden="true">
                  {entry.agentKind
                    ? agentCatalogEntry(entry.agentKind, entry.agentKind, "").glyph
                    : "✦"}
                </span>
                <span class="min-w-0 flex-1 truncate text-[12px] font-medium text-[var(--arena-fg)]">
                  {runLabel(entry)}
                </span>
                <span class="text-[10px] text-[var(--arena-fg-subtle)]">{batchRunStatus(entry)}</span>
              </div>
            {/each}
          </div>
        {:else}
        <div class="flex flex-col gap-1.5">
          {#each reviewers as r (r.id)}
            {@const state = reviewerState(r)}
            <div
              class="flex items-center gap-2.5 rounded-md border px-2.5 py-2 transition-colors
                {state === 'thinking'
                ? 'border-[var(--arena-periwinkle)] bg-[rgba(127,135,255,0.1)]'
                : 'border-[var(--arena-border)] bg-[var(--arena-bg-0)]'}"
            >
              <span
                class="inline-flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-md text-[#0e1420]"
                style="background: {r.color}"
              >
                <span class="text-[10px] font-bold">◆</span>
              </span>
              <span class="flex-1 text-[12px] font-medium text-[var(--arena-fg)]">{r.name}</span>
              {#if state === "thinking"}
                <span
                  class="inline-flex items-center gap-1 text-[10px] font-semibold"
                  style="color: {r.color}"
                >
                  <span class="arena-pulse-dot h-1.5 w-1.5 rounded-full" style="background: {r.color}"></span>
                  Thinking…
                </span>
              {:else if state === "done"}
                <span class="text-[12px] text-[var(--arena-ok,#4ec9a4)]">✓</span>
              {:else if state === "failed"}
                <span class="text-[10px] text-[var(--arena-err,#ff6b6b)]">Failed</span>
              {:else}
                <span class="text-[10px] text-[var(--arena-fg-subtle)]">Queued</span>
              {/if}
            </div>
          {/each}
        </div>
        {/if}

        {#if !batchMode && isArena && totalRounds > 1}
          <div class="flex gap-1">
            {#each Array(totalRounds) as _, roundIdx (roundIdx)}
              {@const i = roundIdx}
              <span
                class="h-1 flex-1 rounded-sm
                  {i + 1 < runProgress.round
                  ? 'bg-[var(--arena-periwinkle)]'
                  : i + 1 === runProgress.round
                    ? 'bg-[rgba(127,135,255,0.4)]'
                    : 'bg-[var(--arena-bg-3)]'}"
              ></span>
            {/each}
          </div>
        {/if}

        <div class="flex items-center gap-2">
          <span class="flex-1 font-mono text-[10px] text-[var(--arena-fg-subtle)]">
            {elapsedSec.toFixed(1)}s elapsed
          </span>
          <button
            type="button"
            class="rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] px-3 py-1.5 text-[11px] text-[var(--arena-fg)]"
            onclick={onCancel}
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-2)] px-3 py-1.5 text-[11px] text-[var(--arena-fg)]"
            onclick={handleSkip}
          >
            Skip to results
          </button>
        </div>
      </div>
    </div>
  {/if}
{/if}
