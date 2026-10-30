<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { digestLine, runOrdinals, runStatusLabel } from "$lib/arena/arenaView";
  import ArenaHistoryList from "$lib/components/arena/ArenaHistoryList.svelte";

  const summaries = $derived(arena.branchSummaries);
  const arenaRuns = $derived(summaries.filter((s) => s.reviewer_count >= 2));
  const ordinals = $derived(runOrdinals(arenaRuns));
  const latest = $derived(
    arenaRuns.reduce<(typeof arenaRuns)[number] | null>(
      (best, s) => (!best || (ordinals.get(s.id) ?? 0) > (ordinals.get(best.id) ?? 0) ? s : best),
      null,
    ),
  );
  const running = $derived(arena.hasLiveRun);
  const digest = $derived(latest ? arena.digests[latest.id] : undefined);
  const activeRunId = $derived(app.snapshot?.active_arena_run ?? arena.liveRunId);

  const line = $derived.by(() => {
    if (!latest) return "Several reviewers vote on the same diff; an arbiter rules on what survives.";
    const n = ordinals.get(latest.id);
    if (latest.status !== "complete") return `Run #${n} · ${runStatusLabel(latest.status)}`;
    if (digest) return `${digestLine(n, digest)}.`;
    return `Run #${n} · ${latest.reviewer_count} reviewers · ${latest.finding_count} findings`;
  });

  // The summary lacks rounds and survivors; one load of the latest run fills them in.
  $effect(() => {
    if (latest && latest.status === "complete" && !arena.digests[latest.id]) void arena.loadDigest(latest.id);
  });
</script>

<div class="rp-arena">
  <div class="rp-arena-h">
    <span>AI review arena</span>
    <span class="links">
      <button type="button" class="linkbtn" onclick={() => arena.openSingleReviewLauncher()}>Single review</button>
      <button type="button" class="linkbtn" onclick={() => arena.openLauncher()}>New arena</button>
    </span>
  </div>
  <p class="dim">{line}</p>
  {#if running}
    <button type="button" class="btn" onclick={() => arena.showRunningProgress()}>View progress</button>
  {:else if latest}
    <button type="button" class="btn" onclick={() => void arena.openOverlay(latest.id)}>Open arena</button>
  {/if}
  {#if summaries.length > 0}
    <details class="hist">
      <summary>History · {summaries.length}</summary>
      <ArenaHistoryList
        {summaries}
        {activeRunId}
        onOpen={(id) => void arena.openOverlay(id)}
        onDelete={(id) => void arena.deleteRun(id)}
      />
    </details>
  {/if}
</div>

<style>
  .rp-arena {
    border: 1px solid var(--color-hairline);
    border-radius: 9px;
    padding: 10px 12px;
    display: grid;
    gap: 6px;
    justify-items: start;
    background: var(--color-surface);
    margin-bottom: 16px;
  }
  .rp-arena-h {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-periwinkle);
  }
  .links {
    display: inline-flex;
    gap: 4px;
  }
  .linkbtn {
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
    color: var(--color-fg-3);
    padding: 2px 6px;
    border-radius: 5px;
    border: 1px solid var(--color-border);
  }
  .linkbtn:hover {
    color: var(--color-fg);
  }
  p {
    margin: 0;
    font-size: 12px;
  }
  .dim {
    color: var(--color-fg-3);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    height: 26px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-fg);
    font-size: 12px;
    font-weight: 500;
  }
  .btn:hover {
    border-color: color-mix(in srgb, var(--color-fg) 25%, transparent);
  }
  .hist {
    width: 100%;
    font-size: 11px;
    color: var(--color-muted);
  }
  .hist summary {
    cursor: pointer;
  }
</style>
