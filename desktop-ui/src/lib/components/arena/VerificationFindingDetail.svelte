<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import type { ArenaFinding, Reviewer } from "$lib/types/arena";
  import { latestSeverity, verdictKey } from "$lib/arena/arenaView";
  import {
    lensNames,
    regrade,
    severityKey,
    splitReasoning,
    type ExcerptLine,
  } from "$lib/arena/verificationView";
  import ArenaVerdictPill from "$lib/components/arena/ArenaVerdictPill.svelte";

  interface Props {
    finding: ArenaFinding;
    /** Short labels ("F3") keyed by finding id, shared with every other view of the run. */
    labels: Map<string, string>;
    reviewers: Reviewer[];
    excerpt: ExcerptLine[];
    accepted: boolean;
    /** Left out of the pending accept; only meaningful while results await it. */
    excluded?: boolean;
    onOpenInDiff: (path: string) => void;
    /** Absent where the run's origin cannot record an override. */
    onKeepAnyway?: (id: string) => void;
    /** Absent unless the results still wait for an accept. */
    onToggleExcluded?: (id: string) => void;
  }

  const {
    finding,
    labels,
    reviewers,
    excerpt,
    accepted,
    excluded = false,
    onOpenInDiff,
    onKeepAnyway,
    onToggleExcluded,
  }: Props = $props();

  const openIds = new SvelteSet<string>();

  const verdict = $derived(verdictKey(finding.verdict));
  const mergedInto = $derived(
    typeof finding.verdict === "object" ? labels.get(finding.verdict.merged.into) ?? finding.verdict.merged.into : null,
  );
  const sev = $derived(latestSeverity(finding));
  const regraded = $derived(regrade(finding));
  const confidence = $derived(Math.round(finding.confidence * 100));
  const lens = $derived(lensNames(finding, reviewers));
  const reasoning = $derived(splitReasoning(finding));
  const fullOpen = $derived(openIds.has(finding.id));
  const location = $derived(`${finding.file}${finding.line ? `:${finding.line}` : ""}`);
  const notes = $derived((finding.evidence ?? []).filter((e) => e.note.trim()));
  const survives = $derived(verdict === "kept" || verdict === "escalated");

  function toggleFull() {
    if (openIds.has(finding.id)) openIds.delete(finding.id);
    else openIds.add(finding.id);
  }
</script>

<article class="vf-detail" aria-label="Finding detail">
  <div class="vf-dh">
    <span class="mono faint">{labels.get(finding.id)}</span>
    <ArenaVerdictPill verdict={finding.verdict} />
    {#if mergedInto}<span class="faint">into <span class="mono">{mergedInto}</span></span>{/if}
    <span class="rlevel lv-{severityKey(sev)}"><i></i>{sev}</span>
    {#if regraded}<span class="chip warn">regraded {regraded}</span>{/if}
    {#if accepted}<span class="chip ok">In review</span>{/if}
    {#if !accepted && excluded}<span class="chip">Left out</span>{/if}
    <span class="flex-1"></span>
    <span class="vf-meter" title="Arbiter confidence">
      <span>confidence</span><b>{confidence}%</b><i><em style="width:{confidence}%"></em></i>
    </span>
  </div>
  <h3 class:struck={verdict === "dropped"}>{finding.title}</h3>
  <button type="button" class="ref mono" onclick={() => onOpenInDiff(finding.file)}>{location}</button>

  <div class="vf-cmp">
    <section>
      <div class="sec">{lens} claimed</div>
      <p>{finding.body || finding.title}</p>
    </section>
    <section class="arb">
      <div class="sec">Why this verdict</div>
      <p>{reasoning.why || "The arbiter has not ruled on this finding yet."}</p>
    </section>
  </div>

  {#if excerpt.length > 0 || notes.length > 0}
    <div class="sec">Evidence</div>
    {#if excerpt.length > 0}
      <pre class="vf-ex">{#each excerpt as l, i (l.num)}{#if i > 0}{"\n"}{/if}<span class="ln">{l.num}</span>{l.text}{/each}</pre>
    {/if}
    {#each notes as e, i (i)}
      <p class="ev">
        <span class="mono faint">{e.file.split("/").pop()}{#if e.line_start}:{e.line_start}{#if e.line_end && e.line_end !== e.line_start}–{e.line_end}{/if}{/if}</span>
        {e.note}
      </p>
    {/each}
  {/if}

  {#if reasoning.full}
    <button type="button" class="vp" aria-expanded={fullOpen} onclick={toggleFull}>
      {fullOpen ? "▾ Hide full reasoning" : "▸ Read full reasoning"}
    </button>
    {#if fullOpen}<p class="vf-full">{reasoning.full}</p>{/if}
  {/if}

  {#if finding.override}
    <div class="sec">Your override</div>
    <p class="override">
      <ArenaVerdictPill verdict={finding.override.verdict} />
      <span>{finding.override.note}</span>
    </p>
  {/if}

  <div class="acts">
    <button type="button" class="btn" onclick={() => onOpenInDiff(finding.file)}>Open in diff</button>
    {#if !accepted}
      {#if !survives && verdict !== "pending" && onKeepAnyway}
        <button type="button" class="btn ghost" onclick={() => onKeepAnyway(finding.id)}>Keep anyway</button>
      {:else if survives && onToggleExcluded}
        <button type="button" class="btn ghost" onclick={() => onToggleExcluded(finding.id)}>
          {excluded ? "Keep" : "Drop"}
        </button>
      {/if}
    {/if}
    <span class="flex-1"></span>
    <span class="faint hint"><kbd class="mono">j</kbd>/<kbd class="mono">k</kbd> next finding</span>
  </div>
</article>

<style>
  .vf-detail {
    overflow: auto;
    padding: 22px 28px 40px;
    display: grid;
    gap: 10px;
    align-content: start;
    grid-auto-rows: max-content;
    max-width: 860px;
    min-height: 0;
  }
  .vf-dh {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  h3 {
    margin: 2px 0 0;
    font-size: 19px;
    font-weight: 600;
    letter-spacing: -0.01em;
    text-wrap: balance;
  }
  h3.struck {
    color: var(--color-muted);
    text-decoration: line-through;
  }
  .faint {
    color: var(--color-muted);
  }
  .rlevel {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 500;
  }
  .rlevel i {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }
  .lv-high {
    color: var(--color-risk-high);
  }
  .lv-high i {
    background: var(--color-risk-high);
  }
  .lv-med {
    color: var(--color-risk-med);
  }
  .lv-med i {
    background: var(--color-risk-med);
  }
  .lv-low {
    color: var(--color-risk-low);
  }
  .lv-low i {
    background: var(--color-risk-low);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 7px;
    border-radius: 5px;
    font-size: 11px;
    font-weight: 500;
    color: var(--color-fg-3);
    background: var(--color-card);
  }
  .chip.warn {
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 14%, transparent);
  }
  .chip.ok {
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 13%, transparent);
  }
  .vf-meter {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--color-muted);
  }
  .vf-meter b {
    font-family: var(--font-mono);
    color: var(--color-fg);
    font-weight: 500;
  }
  .vf-meter i {
    width: 70px;
    height: 4px;
    border-radius: 2px;
    background: var(--color-card);
    overflow: hidden;
    display: block;
  }
  .vf-meter em {
    display: block;
    height: 100%;
    background: var(--color-accent);
  }
  .ref {
    justify-self: start;
    font-size: 12px;
    color: var(--color-fg);
    text-align: left;
    word-break: break-all;
    text-decoration: underline dotted color-mix(in srgb, var(--color-fg) 40%, transparent);
    text-underline-offset: 3px;
  }
  .ref:hover {
    color: var(--color-accent);
  }
  .sec {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--color-muted);
    margin-top: 8px;
  }
  .vf-cmp {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    gap: 10px;
    margin-top: 6px;
  }
  .vf-cmp section {
    border: 1px solid var(--color-hairline);
    border-radius: 9px;
    padding: 10px 12px;
    background: var(--color-surface);
  }
  .vf-cmp section.arb {
    border-left: 2px solid var(--color-fg-3);
  }
  .vf-cmp .sec {
    margin-top: 0;
  }
  .vf-cmp p,
  .vf-full {
    margin: 4px 0 0;
    font-size: 13px;
    line-height: 1.6;
    max-width: 70ch;
    white-space: pre-line;
  }
  .vf-full {
    color: var(--color-fg-3);
  }
  .vf-ex {
    margin: 0;
    padding: 10px 12px;
    border: 1px solid var(--color-hairline);
    border-radius: 8px;
    background: var(--color-surface);
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.7;
    overflow-x: auto;
  }
  .vf-ex .ln {
    display: inline-block;
    width: 48px;
    color: var(--color-muted);
    user-select: none;
  }
  .ev {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--color-fg-3);
    display: flex;
    gap: 8px;
  }
  .vp {
    justify-self: start;
    font-size: 12px;
    color: var(--color-fg-3);
    padding: 2px 4px;
    border-radius: 5px;
  }
  .vp:hover {
    color: var(--color-fg);
    background: var(--color-card);
  }
  .override {
    margin: 0;
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--color-fg-3);
  }
  .acts {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    align-items: center;
    margin-top: 6px;
  }
  .hint {
    font-size: 11.5px;
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
  @media (max-width: 1000px) {
    .vf-cmp {
      grid-template-columns: 1fr;
    }
  }
</style>
