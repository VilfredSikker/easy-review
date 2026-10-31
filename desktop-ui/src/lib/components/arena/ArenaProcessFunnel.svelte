<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { funnelSteps, pageStats } from "$lib/arena/arenaView";

  interface Props {
    snapshot: ArenaRunSnapshot;
  }

  const { snapshot }: Props = $props();

  const steps = $derived(funnelSteps(snapshot));
  const total = $derived(Math.max(1, snapshot.run.findings.length));
  const stats = $derived(pageStats(snapshot.run.findings));
  const split = $derived(
    [
      { label: "Kept", n: stats.kept, color: "var(--color-success)" },
      { label: "Escalated", n: stats.escalated, color: "var(--color-error)" },
      { label: "Merged", n: stats.merged, color: "var(--color-periwinkle)" },
      { label: "Dropped", n: stats.dropped, color: "var(--color-muted)" },
      { label: "Pending", n: stats.pending, color: "var(--color-warning)" },
    ].filter((s) => s.n > 0),
  );
</script>

<div class="funnel">
  {#each steps as step, i (step.label)}
    <div class="fstep">
      <span class="fl">{step.label}</span>
      <div class="fbar" role="img" aria-label="{step.label}: {step.count}">
        <i style="width:{(step.count / total) * 100}%;opacity:{Math.max(0.4, 1 - i * 0.12)}"></i>
        <b>{step.count}</b>
      </div>
      <span class="faint cap">{step.caption}</span>
    </div>
  {/each}
  {#if split.length > 0}
    <div class="fsplit">
      {#each split as s (s.label)}
        <span style="flex:{s.n}" title="{s.label} {s.n}"><i style="background:{s.color}"></i>{s.label} {s.n}</span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .funnel {
    display: grid;
    gap: 10px;
    padding: 8px 0;
    max-width: 820px;
  }
  .fstep {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr) 220px;
    gap: 12px;
    align-items: center;
    font-size: 12.5px;
    color: var(--color-fg);
  }
  .fbar {
    position: relative;
    height: 26px;
    border-radius: 6px;
    background: var(--color-card);
    overflow: hidden;
  }
  .fbar i {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--color-periwinkle);
    border-radius: 6px;
  }
  .fbar b {
    position: absolute;
    left: 10px;
    top: 4px;
    font-family: var(--font-mono);
    font-weight: 500;
    color: var(--color-on-accent);
  }
  .cap {
    font-size: 12px;
  }
  .faint {
    color: var(--color-muted);
  }
  .fsplit {
    display: flex;
    gap: 4px;
    margin-left: 132px;
    margin-right: 232px;
    font-size: 11.5px;
  }
  .fsplit span {
    display: grid;
    gap: 4px;
    color: var(--color-fg-3);
    min-width: 0;
    white-space: nowrap;
  }
  .fsplit i {
    height: 6px;
    border-radius: 3px;
    display: block;
  }
  @media (max-width: 1100px) {
    .fstep {
      grid-template-columns: 100px minmax(0, 1fr);
    }
    .cap {
      display: none;
    }
    .fsplit {
      margin: 0;
    }
  }
</style>
