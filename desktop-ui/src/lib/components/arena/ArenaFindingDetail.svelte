<script lang="ts">
  import type { ArenaFinding, ArenaRunSnapshot } from "$lib/types/arena";
  import { basename, severityTone, verdictLabel, verdictPillClass, voteGlyph } from "$lib/arena/display";
  import ArenaVoteIcon from "$lib/components/arena/ArenaVoteIcon.svelte";

  interface Props {
    snapshot: ArenaRunSnapshot;
    findingId: string | null;
    onClose: () => void;
  }

  const { snapshot, findingId, onClose }: Props = $props();

  const finding = $derived(
    findingId ? snapshot.run.findings.find((f) => f.id === findingId) ?? null : null,
  );

  const reviewerMap = $derived(
    Object.fromEntries(snapshot.run.reviewers.map((r) => [r.id, r])),
  );

  function latestSeverity(f: ArenaFinding): string {
    const rounds = Object.keys(f.severity_by_round)
      .map(Number)
      .sort((a, b) => b - a);
    const last = rounds[0];
    return last !== undefined ? f.severity_by_round[last] : "low";
  }
</script>

{#if finding}
  <aside
    class="flex min-h-0 w-full flex-col border-t border-[var(--arena-border)] bg-[var(--arena-bg-1)] lg:border-t-0 lg:border-l"
    aria-label="Finding detail"
  >
    <header class="flex shrink-0 items-center justify-between border-b border-[var(--arena-border)] px-4 py-2.5">
      <h3 class="text-[12px] font-semibold text-[var(--arena-fg)]">Finding detail</h3>
      <button
        type="button"
        class="text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]"
        onclick={onClose}
        aria-label="Close detail"
      >✕</button>
    </header>
    <div class="min-h-0 flex-1 space-y-3 overflow-y-auto px-4 py-3 text-[12px]">
      <div>
        <p class="font-medium text-[var(--arena-fg)]">{finding.title}</p>
        <p class="mono mt-0.5 text-[10px] text-[var(--arena-fg-subtle)]">
          {basename(finding.file)}{#if finding.line}:{finding.line}{/if}
          · <span class={severityTone(latestSeverity(finding) as "high")}>{latestSeverity(finding)}</span>
        </p>
      </div>

      {#if finding.body}
        <section>
          <p class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">Description</p>
          <p class="mt-1 leading-relaxed text-[var(--arena-fg-muted)]">{finding.body}</p>
        </section>
      {/if}

      <section>
        <p class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">Verdict</p>
        <div class="mt-1 flex flex-wrap items-center gap-2">
          <span class={verdictPillClass(finding.verdict)}>{verdictLabel(finding.verdict)}</span>
          <span class="mono text-[10px] text-[var(--arena-fg-subtle)]"
            >{Math.round(finding.confidence * 100)}%</span
          >
        </div>
        {#if snapshot.arbiter}
          <p class="mt-1 text-[10px] text-[var(--arena-fg-subtle)]">
            Arbiter: {snapshot.arbiter.label}
          </p>
        {/if}
        {#if finding.rationale}
          <p class="mt-2 rounded border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-2 py-1.5 text-[11px] leading-snug text-[var(--arena-fg-muted)]">
            {finding.rationale}
          </p>
        {/if}
      </section>

      {#each finding.rounds as round (round.n)}
        <section>
          <p class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">
            {round.n === 255 ? "Arbiter" : `Round ${round.n}`}
          </p>
          <ul class="mt-1 space-y-1.5">
            {#each round.log as b (b.reviewer + b.vote)}
              {@const agent = reviewerMap[b.reviewer]}
              <li class="flex gap-2 text-[11px] text-[var(--arena-fg-muted)]">
                <span class="mt-0.5 shrink-0">
                  {#if round.n === 255}
                    <ArenaVoteIcon vote={b.vote} size={14} />
                  {:else}
                    <span class="mono w-3 text-center">{voteGlyph(b.vote)}</span>
                  {/if}
                </span>
                <div class="min-w-0 flex-1">
                  <span
                    class="font-semibold"
                    style={agent ? `color:${agent.color}` : ""}
                  >{agent?.name ?? b.reviewer}</span>
                  {#if b.note}
                    <p class="mt-0.5 leading-snug">{b.note}</p>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  </aside>
{/if}
