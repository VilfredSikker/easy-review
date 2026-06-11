<script lang="ts">
  import type { ArenaFinding, ArenaRunSnapshot } from "$lib/types/arena";
  import { arenaStats } from "$lib/arena/projections";
  import { basename, severityTone, verdictLabel, verdictPillClass } from "$lib/arena/display";

  import { arena } from "$lib/stores/arena.svelte";

  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  let accepting = $state(false);

  let expandedIds = $state<Set<string>>(new Set());

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

  function toggleExpand(id: string, e: MouseEvent) {
    e.stopPropagation();
    const next = new Set(expandedIds);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expandedIds = next;
  }

  function isExpanded(f: ArenaFinding): boolean {
    return expandedIds.has(f.id) || selectedId === f.id;
  }

  const pendingAccept = $derived(
    truthFindings.filter((f) => !f.accepted_at && !snapshot.run.accepted_finding_ids?.includes(f.id)),
  );

  async function acceptAll() {
    if (pendingAccept.length === 0) return;
    accepting = true;
    try {
      await arena.acceptFindings(
        snapshot.run.id,
        pendingAccept.map((f) => f.id),
      );
    } finally {
      accepting = false;
    }
  }

  async function acceptOne(id: string, e: MouseEvent) {
    e.stopPropagation();
    accepting = true;
    try {
      await arena.acceptFindings(snapshot.run.id, [id]);
    } finally {
      accepting = false;
    }
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
    {#if pendingAccept.length > 0}
      <button
        type="button"
        disabled={accepting}
        class="mt-2 inline-flex w-full items-center justify-center gap-1.5 rounded-md bg-[var(--arena-periwinkle)] px-3 py-1.5 text-[11px] font-semibold text-[var(--arena-bg-0)] disabled:opacity-50"
        onclick={() => void acceptAll()}
      >
        Accept all into Review ({pendingAccept.length})
      </button>
    {/if}
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-3 py-2">
    {#each ["high", "med", "low"] as sev (sev)}
      {@const list = sev === "high" ? bySeverity.high : sev === "med" ? bySeverity.med : bySeverity.low}
      {#if list.length > 0}
        <p class="mb-1 mt-3 px-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">
          {sev}
        </p>
        {#each list as f (f.id)}
          {@const expanded = isExpanded(f)}
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
                {#if f.accepted_at || snapshot.run.accepted_finding_ids?.includes(f.id)}
                  <span class="mt-1 inline-block text-[9px] font-semibold uppercase tracking-wider text-[var(--arena-ok)]">
                    In Review
                  </span>
                {:else}
                  <button
                    type="button"
                    disabled={accepting}
                    class="mt-1 text-[10px] font-semibold text-[var(--arena-periwinkle)] hover:underline disabled:opacity-50"
                    onclick={(e) => void acceptOne(f.id, e)}
                  >
                    Accept into Review
                  </button>
                {/if}
                {#if f.rationale}
                  <div class="mt-1.5">
                    <p
                      class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]"
                    >
                      Why this verdict
                    </p>
                    <p
                      class="mt-0.5 rounded border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-2 py-1.5 text-[11px] leading-snug text-[var(--arena-fg-muted)] {expanded
                        ? ''
                        : 'line-clamp-3'}"
                    >
                      {f.rationale}
                    </p>
                    {#if f.rationale.length > 120}
                      <button
                        type="button"
                        class="mt-1 text-[10px] font-semibold text-[var(--arena-periwinkle)] hover:underline"
                        onclick={(e) => toggleExpand(f.id, e)}
                      >
                        {expanded ? "Show less" : "Read full reasoning"}
                      </button>
                    {/if}
                  </div>
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
