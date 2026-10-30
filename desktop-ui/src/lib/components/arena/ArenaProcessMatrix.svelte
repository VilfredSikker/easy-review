<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { basename } from "$lib/arena/display";
  import {
    VOTE_META,
    arbiterBallot,
    findingLabels,
    firstSentence,
    hasArbiterRound,
    latestSeverity,
    reviewerColorVar,
    verdictKey,
    votesAtRound,
  } from "$lib/arena/arenaView";
  import ArenaVoteCell from "$lib/components/arena/ArenaVoteCell.svelte";
  import ArenaVerdictPill from "$lib/components/arena/ArenaVerdictPill.svelte";

  interface Props {
    snapshot: ArenaRunSnapshot;
    /** Reviewer round whose ballots fill the vote cells. */
    round: number;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, round, selectedId, onSelect }: Props = $props();

  const reviewers = $derived(snapshot.run.reviewers);
  const labels = $derived(findingLabels(snapshot.run.findings));
  const showArbiter = $derived(hasArbiterRound(snapshot));
</script>

<div class="wrap">
  <table class="ar-table">
    <thead>
      <tr>
        <th>Finding</th>
        {#each reviewers as r, ri (r.id)}
          <th class="c" title="{r.name} · {r.model_id}"><i style="background:{reviewerColorVar(r, ri)}"></i>{r.name}</th>
        {/each}
        {#if showArbiter}<th>Arbiter</th>{/if}
        <th>Verdict</th>
      </tr>
    </thead>
    <tbody>
      {#each snapshot.run.findings as f (f.id)}
        {@const votes = votesAtRound(f, round)}
        {@const ruling = f.rationale || arbiterBallot(f)?.note || ""}
        <tr
          class:dropped={verdictKey(f.verdict) === "dropped"}
          class:sel={selectedId === f.id}
          onclick={() => onSelect(f.id)}
        >
          <td>
            <button type="button" class="ft" aria-pressed={selectedId === f.id}>
              <span class="mono faint">{labels.get(f.id)}</span>
              <span class="sev sev-{latestSeverity(f)}" title="{latestSeverity(f)} severity"></span>
              <span class="t">{f.title}</span>
            </button>
            <div class="mono faint loc">{basename(f.file)}{f.line ? `:${f.line}` : ""}</div>
          </td>
          {#each reviewers as r (r.id)}
            {@const v = votes[r.id]}
            <td class="c">
              <ArenaVoteCell
                vote={v}
                title="{r.name} · round {round}: {v ? VOTE_META[v].label : 'no vote'}"
              />
            </td>
          {/each}
          {#if showArbiter}
            <td title={ruling}><span class="arb">{ruling ? firstSentence(ruling) : "—"}</span></td>
          {/if}
          <td><ArenaVerdictPill verdict={f.verdict} /></td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .wrap {
    overflow-x: auto;
    border: 1px solid var(--color-hairline);
    border-radius: 10px;
    background: var(--color-surface);
  }
  .ar-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12.5px;
  }
  th {
    text-align: left;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--color-muted);
    padding: 9px 10px;
    border-bottom: 1px solid var(--color-hairline);
    white-space: nowrap;
  }
  th i {
    display: inline-block;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    margin-right: 5px;
  }
  td {
    padding: 8px 10px;
    border-bottom: 1px solid var(--color-hairline);
    vertical-align: middle;
    color: var(--color-fg);
  }
  tr:last-child td {
    border-bottom: 0;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover td {
    background: var(--color-card);
  }
  .ft:focus-visible {
    outline: 1px solid var(--color-accent);
    outline-offset: 2px;
    border-radius: 3px;
  }
  tr.sel td {
    background: color-mix(in srgb, var(--color-accent) 9%, var(--color-surface));
  }
  tr.sel td:first-child {
    box-shadow: inset 2px 0 0 var(--color-accent);
  }
  tr.dropped .t {
    text-decoration: line-through;
    color: var(--color-muted);
  }
  .c {
    text-align: center;
  }
  .ft {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    text-align: left;
    color: inherit;
  }
  .t {
    font-weight: 500;
  }
  .loc {
    font-size: 10.5px;
    margin-top: 2px;
    padding-left: 36px;
  }
  .faint {
    color: var(--color-muted);
  }
  .arb {
    color: var(--color-fg-3);
    font-size: 12px;
    max-width: 220px;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .sev {
    width: 7px;
    height: 7px;
    border-radius: 2px;
    flex: none;
    background: var(--color-muted);
  }
  .sev-high {
    background: var(--color-risk-high);
  }
  .sev-med {
    background: var(--color-risk-med);
  }
  .sev-low {
    background: var(--color-risk-low);
  }
</style>
