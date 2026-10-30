<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import {
    VOTE_META,
    arbiterBallot,
    ballotTrail,
    findingLabels,
    isAccepted,
    reviewerColorVar,
    roundNumbers,
    verdictKey,
  } from "$lib/arena/arenaView";
  import ArenaVoteCell from "$lib/components/arena/ArenaVoteCell.svelte";
  import ArenaVerdictPill from "$lib/components/arena/ArenaVerdictPill.svelte";

  interface Props {
    snapshot: ArenaRunSnapshot;
    findingId: string | null;
    /** Flow and verification results are read only: no override, no exclusion. */
    readOnly?: boolean;
    excluded?: boolean;
    onToggleExcluded?: (id: string) => void;
    onKeepAnyway?: (id: string) => void;
    onOpenInDiff?: (path: string) => void;
  }

  const {
    snapshot,
    findingId,
    readOnly = false,
    excluded = false,
    onToggleExcluded,
    onKeepAnyway,
    onOpenInDiff,
  }: Props = $props();

  const finding = $derived(
    findingId ? snapshot.run.findings.find((f) => f.id === findingId) ?? null : null,
  );
  const labels = $derived(findingLabels(snapshot.run.findings));
  const rounds = $derived(roundNumbers(snapshot));
  const reviewers = $derived(snapshot.run.reviewers.map((r, i) => ({ r, color: reviewerColorVar(r, i) })));
  const proposers = $derived(
    finding ? reviewers.filter(({ r }) => finding.raised_by.includes(r.id)) : [],
  );
  const verdict = $derived(finding ? verdictKey(finding.verdict) : "pending");
  const ruling = $derived(finding ? finding.rationale || arbiterBallot(finding)?.note || "" : "");
  const arbiterVote = $derived(finding ? arbiterBallot(finding)?.vote : undefined);
  const accepted = $derived(finding ? isAccepted(snapshot.run, finding) : false);
  const location = $derived(finding ? `${finding.file}${finding.line ? `:${finding.line}` : ""}` : "");

  /** The last thing a reviewer said about this finding, quoted under their votes. */
  function lastNote(reviewerId: string): string {
    if (!finding) return "";
    const trail = ballotTrail(finding, reviewerId, rounds);
    for (let i = trail.length - 1; i >= 0; i--) {
      const note = trail[i]?.note?.trim();
      if (note) return note;
    }
    return "";
  }
</script>

{#if finding}
  <aside class="ar-detail" aria-label="Finding detail">
    <div class="head">
      <span class="mono faint">{labels.get(finding.id)}</span>
      <ArenaVerdictPill verdict={finding.verdict} />
      {#if accepted}<span class="chip ok">In review</span>{/if}
      {#if !accepted && excluded}<span class="chip">Left out of import</span>{/if}
    </div>
    <h3>{finding.title}</h3>
    {#if onOpenInDiff}
      <button type="button" class="ref mono" onclick={() => onOpenInDiff(finding.file)}>{location}</button>
    {:else}
      <span class="ref mono">{location}</span>
    {/if}
    {#if finding.body}
      <p class="body">{finding.body}</p>
    {/if}

    {#if proposers.length > 0}
      <div class="sec">Proposed by</div>
      <div class="revs">
        {#each proposers as { r, color } (r.id)}
          <span class="rev"><i style="background:{color}"></i>{r.name}{r.kind === "agent" ? "" : ` · ${r.model_id}`}</span>
        {/each}
      </div>
    {/if}

    <div class="sec">Votes by round</div>
    <div class="votes">
      {#each reviewers as { r, color } (r.id)}
        {@const trail = ballotTrail(finding, r.id, rounds)}
        {@const note = lastNote(r.id)}
        <div class="vrow">
          <span class="rev"><i style="background:{color}"></i>{r.name}</span>
          <span class="vs">
            {#each trail as ballot, i (i)}
              {#if i > 0}<span class="faint">›</span>{/if}
              <ArenaVoteCell
                vote={ballot?.vote}
                title="Round {rounds[i]}: {ballot ? VOTE_META[ballot.vote].label : 'no vote'}{ballot?.note ? ` — ${ballot.note}` : ''}"
              />
            {/each}
          </span>
        </div>
        {#if note}<p class="why">“{note}”</p>{/if}
      {/each}
    </div>

    {#if ruling}
      <div class="sec">
        Arbiter ruling{#if snapshot.arbiter}<span class="faint norm"> · {snapshot.arbiter.label}</span>{/if}
      </div>
      <blockquote class="ruling">
        {#if arbiterVote}<ArenaVoteCell vote={arbiterVote} />{/if}
        {ruling}
      </blockquote>
      <p class="faint caption">Recorded beside the finding, not over it, so the disagreement stays readable.</p>
    {/if}

    {#if finding.override}
      <div class="sec">Your override</div>
      <p class="override">
        <ArenaVerdictPill verdict={finding.override.verdict} />
        <span>{finding.override.note}</span>
      </p>
    {/if}

    <div class="acts">
      {#if onOpenInDiff}
        <button type="button" class="btn" onclick={() => onOpenInDiff(finding.file)}>Open in diff</button>
      {/if}
      {#if !readOnly && !accepted}
        {#if (verdict === "dropped" || verdict === "merged") && onKeepAnyway}
          <button type="button" class="btn ghost" onclick={() => onKeepAnyway(finding.id)}>Keep anyway</button>
        {:else if (verdict === "kept" || verdict === "escalated") && onToggleExcluded}
          <button type="button" class="btn ghost" onclick={() => onToggleExcluded(finding.id)}>
            {excluded ? "Include in import" : "Leave out of import"}
          </button>
        {/if}
      {/if}
    </div>
  </aside>
{/if}

<style>
  .ar-detail {
    border-left: 1px solid var(--color-hairline);
    padding: 16px 18px 24px;
    display: grid;
    gap: 8px;
    align-content: start;
    overflow: auto;
    background: var(--color-bg);
    color: var(--color-fg);
    font-size: 12.5px;
    min-height: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.35;
    text-wrap: balance;
  }
  .ref {
    justify-self: start;
    font-size: 11.5px;
    color: var(--color-action);
    text-align: left;
    word-break: break-all;
  }
  button.ref:hover {
    text-decoration: underline;
  }
  .body {
    margin: 0;
    color: var(--color-fg-3);
    line-height: 1.5;
  }
  .sec {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-muted);
    margin-top: 8px;
  }
  .norm {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
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
    min-width: 0;
  }
  .rev i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex: none;
  }
  .votes {
    display: grid;
    gap: 4px;
  }
  .vrow {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .vs {
    display: inline-flex;
    gap: 3px;
    align-items: center;
  }
  .why {
    margin: 0 0 4px 14px;
    font-size: 11.5px;
    color: var(--color-fg-3);
    font-style: italic;
  }
  .ruling {
    margin: 0;
    padding: 8px 12px;
    border-left: 2px solid var(--color-fg-3);
    background: var(--color-surface);
    border-radius: 0 7px 7px 0;
    line-height: 1.55;
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .caption {
    font-size: 11.5px;
    margin: 0;
  }
  .override {
    margin: 0;
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--color-fg-3);
  }
  .faint {
    color: var(--color-muted);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 19px;
    padding: 0 7px;
    border-radius: 5px;
    font-size: 11px;
    font-weight: 500;
    color: var(--color-fg-3);
    background: var(--color-card);
  }
  .chip.ok {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 13%, transparent);
  }
  .acts {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 6px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-fg);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn:hover {
    border-color: color-mix(in srgb, var(--color-fg) 25%, transparent);
  }
  .btn.ghost {
    border-color: transparent;
    background: none;
    color: var(--color-fg-3);
  }
  .btn.ghost:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
</style>
