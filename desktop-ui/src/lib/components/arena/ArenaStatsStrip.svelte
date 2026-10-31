<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { ARBITER_COLOR, pageStats, reviewerColorVar, roundsRun, runDuration } from "$lib/arena/arenaView";

  interface Props {
    snapshot: ArenaRunSnapshot;
    /** Effort the run used, already labelled; null hides it. */
    effort: string | null;
  }

  const { snapshot, effort }: Props = $props();

  const stats = $derived(pageStats(snapshot.run.findings));
  const cards = $derived([
    { label: "Proposed", n: stats.proposed, color: "var(--color-fg)" },
    { label: "Kept", n: stats.kept, color: "var(--color-success)" },
    { label: "Escalated", n: stats.escalated, color: "var(--color-error)" },
    { label: "Merged", n: stats.merged, color: "var(--color-periwinkle)" },
    { label: "Dropped", n: stats.dropped, color: "var(--color-muted)" },
    ...(stats.pending > 0 ? [{ label: "Pending", n: stats.pending, color: "var(--color-warning)" }] : []),
  ]);
  const rounds = $derived(roundsRun(snapshot.run.findings));
  const duration = $derived(runDuration(snapshot.run.created_at, snapshot.run.completed_at));
  // The engine records an estimate, not the spend, so the figure carries "≈".
  const cost = $derived(snapshot.run.cost_estimate?.usd ?? 0);
  const meta = $derived(
    [
      `${rounds} round${rounds === 1 ? "" : "s"}`,
      effort ? `effort ${effort.toLowerCase()}` : "",
      duration ?? "",
      cost > 0 ? `≈ $${cost.toFixed(2)}` : "",
      snapshot.run.diff_hash ? `built from ${snapshot.run.diff_hash.slice(0, 7)}` : "",
    ].filter(Boolean).join(" · "),
  );
</script>

<div class="ar-stats">
  {#each cards as c (c.label)}
    <div class="ar-stat">
      <span>{c.label}</span>
      <b style="color:{c.color}">{c.n}</b>
    </div>
  {/each}
  <div class="ar-meta">
    <div class="revs">
      {#each snapshot.run.reviewers as r, i (r.id)}
        <span class="rev" title="{r.name} · {r.model_id}"><i style="background:{reviewerColorVar(r, i)}"></i>{r.name}</span>
      {/each}
      {#if snapshot.arbiter}
        <span class="rev"><i style="background:{ARBITER_COLOR}"></i>Arbiter · {snapshot.arbiter.label}</span>
      {/if}
    </div>
    <div class="mono line">{meta}</div>
  </div>
</div>

<style>
  .ar-stats {
    display: flex;
    gap: 8px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--color-hairline);
    flex-wrap: wrap;
    align-items: stretch;
  }
  .ar-stat {
    display: grid;
    gap: 2px;
    padding: 8px 14px;
    border: 1px solid var(--color-hairline);
    border-radius: 9px;
    background: var(--color-surface);
    min-width: 92px;
  }
  .ar-stat span {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-muted);
    font-weight: 600;
  }
  .ar-stat b {
    font-family: var(--font-mono);
    font-size: 20px;
    font-weight: 500;
  }
  .ar-meta {
    display: grid;
    gap: 6px;
    align-content: center;
    margin-left: 8px;
    min-width: 0;
  }
  .revs {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  .rev {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    white-space: nowrap;
    color: var(--color-fg);
  }
  .rev i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .line {
    font-size: 11px;
    color: var(--color-muted);
  }
</style>
