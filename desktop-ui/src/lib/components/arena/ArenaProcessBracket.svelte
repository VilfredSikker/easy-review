<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import {
    aliveInRound,
    arbiterRuled,
    findingLabels,
    roundNumbers,
    roundsRun,
    survivesArbiter,
  } from "$lib/arena/arenaView";
  import ArenaVerdictPill from "$lib/components/arena/ArenaVerdictPill.svelte";

  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  const findings = $derived(snapshot.run.findings);
  const labels = $derived(findingLabels(findings));
  const rounds = $derived(roundNumbers(snapshot));
  const ran = $derived(roundsRun(findings));
  const ruled = $derived(arbiterRuled(snapshot));

  /** Rounds that ran, then the arbiter once it has ruled. */
  const columns = $derived([
    ...rounds.filter((n) => n <= Math.max(ran, 1)).map((n) => ({ key: `r${n}`, label: `Round ${n}`, round: n })),
    ...(ruled ? [{ key: "arb", label: "Arbiter", round: null }] : []),
  ]);

  function alive(f: (typeof findings)[number], round: number | null): boolean {
    return round === null ? survivesArbiter(f) : aliveInRound(f, round, rounds);
  }
</script>

<div class="bracket" style="grid-template-columns: repeat({columns.length}, minmax(170px, 1fr))">
  {#each columns as col (col.key)}
    <div class="bcol">
      <div class="bcol-h">
        {col.label}<span class="mono faint">{findings.filter((f) => alive(f, col.round)).length}</span>
      </div>
      {#each findings as f (f.id)}
        {@const on = alive(f, col.round)}
        <button
          type="button"
          class="bchip"
          class:out={!on}
          class:sel={selectedId === f.id}
          aria-pressed={selectedId === f.id}
          title={f.title}
          onclick={() => onSelect(f.id)}
        >
          <span class="mono faint">{labels.get(f.id)}</span>
          <span class="t">{f.title}</span>
          {#if col.round === null}
            <span class="tail"><ArenaVerdictPill verdict={f.verdict} /></span>
          {:else if !on}
            <span class="tail faint">out</span>
          {/if}
        </button>
      {/each}
    </div>
  {/each}
</div>

<style>
  .bracket {
    display: grid;
    gap: 10px;
    overflow-x: auto;
  }
  .bcol {
    display: grid;
    gap: 5px;
    align-content: start;
  }
  .bcol-h {
    display: flex;
    justify-content: space-between;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-muted);
    padding: 0 2px 4px;
  }
  .bchip {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 2px 8px;
    padding: 7px 9px;
    border: 1px solid var(--color-hairline);
    border-radius: 8px;
    background: var(--color-surface);
    text-align: left;
    font-size: 12px;
    color: var(--color-fg);
  }
  .bchip:hover {
    border-color: var(--color-border);
  }
  .bchip .t {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tail {
    grid-column: 2;
    justify-self: start;
    font-size: 10.5px;
  }
  .bchip.out {
    opacity: 0.35;
    background: none;
    border-style: dashed;
  }
  .bchip.sel {
    border-color: var(--color-accent);
  }
  .faint {
    color: var(--color-muted);
  }
</style>
