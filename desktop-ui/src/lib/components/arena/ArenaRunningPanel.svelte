<script lang="ts">
  import type { ArenaStartConfig, LiveRunEntry, LiveRunState } from "$lib/stores/arena.svelte";
  import { arena, isArenaRunActive } from "$lib/stores/arena.svelte";
  import { agentCatalogEntry } from "$lib/arena/agents";
  import {
    ARBITER_COLOR,
    type ReviewerRunState,
    reviewerColorVar,
    reviewerProgress,
    runOrdinals,
  } from "$lib/arena/arenaView";
  import type { ArenaProgressState, ArenaRunSnapshot, Reviewer } from "$lib/types/arena";

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

  const isArena = $derived(
    (snapshot?.run.reviewers.length ?? 0) >= 2 ||
      (config?.reviewers?.length ?? 0) >= 2 ||
      (config?.agent_groups?.some((g) => g.models.length >= 2) ?? false),
  );
  const totalRounds = $derived(
    progress?.total_rounds || snapshot?.run.config.rounds || (isArena ? (config?.rounds ?? 3) : 1),
  );
  const arbiterPhase = $derived(progress?.phase === "arbiter");
  const status = $derived(snapshot?.run.status);
  const round = $derived.by(() => {
    if (progress?.round) return progress.round;
    if (status && typeof status === "object" && "running" in status) return status.running.round;
    return 1;
  });
  const hasArbiter = $derived(isArena && totalRounds >= 2);

  const runName = $derived.by(() => {
    const id = snapshot?.run.id;
    if (!id) return isArena ? "New arena" : "Review";
    const tabs = arena.branchSummaries.filter((s) => s.reviewer_count >= 2 || s.id === id);
    const n = runOrdinals(tabs.some((s) => s.id === id) ? tabs : [...tabs, { id, created_at: snapshot.run.created_at }]).get(id);
    return n ? `Run #${n}` : "Run";
  });

  const headline = $derived.by(() => {
    if (batchMode) return `${liveRuns.length} reviews in progress`;
    if (status === "queued") return `${runName} · queued`;
    if (status === "failed") return `${runName} · failed`;
    if (status === "cancelled") return `${runName} · cancelled`;
    if (arbiterPhase) return `${runName} · arbiter ruling`;
    if (!isArena) return "Review in progress";
    return `${runName} · round ${Math.min(round, totalRounds)} of ${totalRounds}`;
  });

  // Shown only when the engine has priced the run; it is an estimate, never the spend.
  const estCost = $derived(snapshot?.run.cost_estimate?.usd ?? 0);

  const reviewers = $derived.by((): Reviewer[] => {
    const fromSnap = snapshot?.run.reviewers ?? [];
    if (fromSnap.length > 0) return fromSnap;
    return (config?.reviewers ?? []).map((r) => ({
      id: r.agent_kind ? `${r.agent_kind}::${r.provider_id}-${r.model_id}` : `${r.provider_id}-${r.model_id}`,
      name: r.agent_kind ? agentCatalogEntry(r.agent_kind, r.agent_kind, "").label : r.model_id,
      kind: r.agent_kind ? ("agent" as const) : ("model" as const),
      provider_id: r.provider_id,
      model_id: r.model_id,
      system_prompt: "",
      color: "",
      icon: "cube",
      tagline: r.provider_id,
      cost_per_1k_in: 0,
      cost_per_1k_out: 0,
      avg_latency_ms: 12_000,
      status: "ok" as const,
      agent_kind: r.agent_kind,
    }));
  });

  function reviewerState(r: Reviewer): ReviewerRunState {
    if (typeof r.status === "object" && r.status !== null && "failed" in r.status) return "failed";
    if (arbiterPhase || status === "complete") return "done";
    if (progress?.done.includes(r.id)) return "done";
    if (progress?.thinking.includes(r.id)) return "running";
    return "queued";
  }

  const STATE_LABEL: Record<ReviewerRunState, string> = {
    queued: "Queued",
    running: "Running",
    done: "✓ done",
    failed: "Failed",
  };

  /** The arbiter rules in one call, so its bar only has three positions. */
  const ARBITER_PCT: Record<ReviewerRunState, number> = { queued: 0, running: 50, done: 100, failed: 0 };

  const arbiterState = $derived.by((): ReviewerRunState => {
    if (status === "complete") return "done";
    return arbiterPhase ? "running" : "queued";
  });

  const overallPct = $derived.by(() => {
    if (reviewers.length === 0) return 0;
    const sum = reviewers.reduce(
      (n, r) => n + reviewerProgress(reviewerState(r), round, totalRounds, arbiterPhase),
      0,
    );
    return Math.round(sum / reviewers.length);
  });

  function runLabel(entry: LiveRunEntry): string {
    if (entry.title) return entry.title;
    if (entry.agentKind) return agentCatalogEntry(entry.agentKind, entry.agentKind, "").label;
    return entry.runId.slice(0, 12);
  }

  function batchRunStatus(entry: LiveRunEntry): string {
    const st = liveRunStates[entry.runId]?.snapshot?.run.status;
    if (!st) return "Starting…";
    if (typeof st === "object" && "running" in st) return `Round ${st.running.round}`;
    return st.charAt(0).toUpperCase() + st.slice(1);
  }

  $effect(() => {
    if (snapshot?.run.status === "complete") {
      onComplete();
    }
  });

  $effect(() => {
    if (!open || !startedAt) {
      elapsedSec = 0;
      return;
    }
    const update = () => {
      elapsedSec = Math.max(0, (Date.now() - startedAt) / 1000);
    };
    update();
    const timer = setInterval(update, 500);
    return () => clearInterval(timer);
  });

  function elapsedLabel(sec: number): string {
    const s = Math.floor(sec);
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
  }
</script>

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
      class="pill fixed bottom-20 right-6 z-[260]"
      title="Open progress"
      aria-label="Open arena progress"
    >
      <span class="pbar pill-bar"><i style="width:{overallPct}%;background:var(--color-periwinkle)"></i></span>
      <span class="min-w-0 flex-1">
        <span class="block truncate pill-t">{headline}</span>
        <span class="block truncate mono faint">{elapsedLabel(elapsedSec)} · {overallPct}%</span>
      </span>
      <button
        type="button"
        class="x"
        aria-label="Cancel run"
        onclick={(e) => {
          e.stopPropagation();
          onCancel();
        }}>✕</button
      >
    </div>
  {:else}
    <div class="scrim fixed inset-0 z-[260]" role="dialog" aria-modal="true" aria-label="Arena in progress">
      <div class="ar-run">
        <div class="ar-run-h">
          <b>{headline}</b>
          <span class="faint mono">
            {elapsedLabel(elapsedSec)}{#if estCost > 0}&nbsp;· est. ${estCost.toFixed(2)}{/if}
          </span>
          <span class="flex-1"></span>
          <button type="button" class="btn ghost" onclick={onMinimize}>Run in background</button>
          <button type="button" class="btn ghost" onclick={onCancel}>Cancel run</button>
        </div>

        {#if batchMode}
          {#each liveRuns as entry (entry.runId)}
            {@const st = liveRunStates[entry.runId]?.snapshot?.run.status}
            <div class="ar-prow">
              <span class="rev">{runLabel(entry)}</span>
              <span class="faint mono">{st != null && isArenaRunActive(st) ? "active" : ""}</span>
              <span></span>
              <span class="mono state">{batchRunStatus(entry)}</span>
            </div>
          {/each}
        {:else}
          {#each reviewers as r, i (r.id)}
            {@const state = reviewerState(r)}
            {@const color = reviewerColorVar(r, i)}
            <div class="ar-prow">
              <span class="rev" title={r.name}><i style="background:{color}"></i><span class="truncate">{r.name}</span></span>
              <span class="faint mono model" title={r.model_id}>{r.model_id}</span>
              <div class="pbar">
                <i style="width:{reviewerProgress(state, round, totalRounds, arbiterPhase)}%;background:{color}"></i>
              </div>
              <span class="mono state" class:ok={state === "done"} class:err={state === "failed"}>{STATE_LABEL[state]}</span>
            </div>
          {/each}
          {#if hasArbiter}
            <div class="ar-prow">
              <span class="rev"><i style="background:{ARBITER_COLOR}"></i>Arbiter</span>
              <span class="faint mono model">{snapshot?.run.config.arbiter.model_id ?? config.arbiter?.model_id ?? ""}</span>
              <div class="pbar">
                <i style="width:{ARBITER_PCT[arbiterState]}%;background:{ARBITER_COLOR}"></i>
              </div>
              <span class="mono state" class:ok={arbiterState === "done"}>{STATE_LABEL[arbiterState]}</span>
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
{/if}

<style>
  .scrim {
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 64px 16px 16px;
    background: color-mix(in srgb, var(--color-ground) 55%, transparent);
    overflow: auto;
  }
  .ar-run {
    width: min(720px, 100%);
    border: 1px solid var(--color-hairline);
    border-radius: 12px;
    background: var(--color-surface);
    padding: 14px 16px;
    display: grid;
    gap: 10px;
    color: var(--color-fg);
    font-size: 12.5px;
    box-shadow: 0 24px 60px color-mix(in srgb, var(--color-ground) 60%, transparent);
  }
  .ar-run-h {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .ar-prow {
    display: grid;
    grid-template-columns: 140px 110px minmax(0, 1fr) 64px;
    gap: 12px;
    align-items: center;
  }
  .rev {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .rev i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .model {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pbar {
    height: 6px;
    border-radius: 3px;
    background: var(--color-hover);
    overflow: hidden;
  }
  .pbar i {
    display: block;
    height: 100%;
    border-radius: 3px;
    transition: width 0.12s linear;
  }
  .state {
    font-size: 11px;
    color: var(--color-fg-3);
  }
  .state.ok {
    color: var(--color-success);
  }
  .state.err {
    color: var(--color-error);
  }
  .faint {
    color: var(--color-muted);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    font-weight: 500;
    white-space: nowrap;
    color: var(--color-fg-3);
  }
  .btn:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 240px;
    max-width: 320px;
    padding: 8px 12px;
    border-radius: 10px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-fg);
    font-size: 11px;
    cursor: pointer;
    box-shadow: 0 10px 28px color-mix(in srgb, var(--color-ground) 50%, transparent);
  }
  .pill:hover {
    border-color: var(--color-periwinkle);
  }
  .pill-bar {
    width: 36px;
    flex: none;
  }
  .pill-t {
    font-weight: 500;
  }
  .x {
    color: var(--color-fg-3);
    padding: 2px 4px;
    border-radius: 5px;
  }
  .x:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
</style>
