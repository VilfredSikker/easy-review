<script lang="ts">
  import type { ArenaFinding, ArenaRunSnapshot } from "$lib/types/arena";
  import { arenaStats } from "$lib/arena/projections";
  import { basename, severityTone, verdictLabel, verdictPillClass } from "$lib/arena/display";
  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  const reviewerMap = $derived(
    Object.fromEntries(snapshot.run.reviewers.map((r) => [r.id, r])),
  );
  const stats = $derived(arenaStats(snapshot.run.findings));

  const truthFindings = $derived(
    snapshot.run.findings.filter((f) => {
      if (f.verdict === "dropped" || f.verdict === "pending") return false;
      return true;
    }),
  );

  const bySeverity = $derived({
    high: truthFindings.filter((f) => latestSeverity(f) === "high"),
    med: truthFindings.filter((f) => latestSeverity(f) === "med"),
    low: truthFindings.filter((f) => {
      const s = latestSeverity(f);
      return s === "low" || s === "info";
    }),
  });

  function latestSeverity(f: ArenaFinding): string {
    const rounds = Object.keys(f.severity_by_round)
      .map(Number)
      .sort((a, b) => b - a);
    const last = rounds[0];
    return last !== undefined ? f.severity_by_round[last] : "low";
  }
</script>

<aside
  class="flex min-h-0 flex-col border-l border-[var(--arena-border)] bg-[var(--arena-bg-1)]"
  aria-label="Final truth"
>
  <header class="shrink-0 border-b border-[var(--arena-border)] px-4 py-3">
    <h2 class="text-[13px] font-semibold text-[var(--arena-fg)]">Final truth</h2>
    <p class="mt-0.5 text-[11px] text-[var(--arena-fg-subtle)]">
      {stats.finalCount} shipped · {stats.verdicts.dropped} dropped
    </p>
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-3 py-2">
    {#each ["high", "med", "low"] as sev (sev)}
      {@const list = sev === "high" ? bySeverity.high : sev === "med" ? bySeverity.med : bySeverity.low}
      {#if list.length > 0}
        <p class="mb-1 mt-3 px-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">
          {sev}
        </p>
        {#each list as f (f.id)}
          <button
            type="button"
            class="arena-link-row mb-1 w-full rounded-md px-2 py-2 text-left {selectedId === f.id ? 'arena-selected' : ''}"
            onclick={() => onSelect(f.id)}
          >
            <div class="flex items-start gap-2">
              <span class="mono mt-0.5 text-[10px] {severityTone(latestSeverity(f) as 'high')}">●</span>
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="text-[12px] font-medium text-[var(--arena-fg)]">{f.title}</span>
                  <span class={verdictPillClass(f.verdict)}>{verdictLabel(f.verdict)}</span>
                  <span class="mono text-[10px] text-[var(--arena-fg-subtle)]"
                    >{Math.round(f.confidence * 100)}%</span
                  >
                </div>
                <p class="mono truncate text-[10px] text-[var(--arena-fg-subtle)]">
                  {basename(f.file)}{#if f.line}:{f.line}{/if}
                </p>
                {#if selectedId === f.id && f.rationale}
                  <p class="mt-1.5 rounded border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-2 py-1.5 text-[11px] leading-snug text-[var(--arena-fg-muted)]">
                    {f.rationale}
                  </p>
                {/if}
                <div class="mt-1 flex flex-wrap gap-1">
                  {#each f.raised_by as rid}
                    {@const r = reviewerMap[rid]}
                    {#if r}
                      <span
                        class="text-[9px] font-semibold"
                        style="color: {r.color}"
                        title={r.name}>{r.name}</span
                      >
                    {/if}
                  {/each}
                </div>
              </div>
            </div>
          </button>
        {/each}
      {/if}
    {/each}
  </div>
</aside>
