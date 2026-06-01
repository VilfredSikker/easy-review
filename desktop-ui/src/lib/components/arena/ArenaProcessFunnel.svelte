<script lang="ts">
  import type { ArenaFinding, ArenaRunSnapshot, FunnelStage } from "$lib/types/arena";
  import { basename } from "$lib/arena/display";

  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  const reviewerMap = $derived(
    Object.fromEntries(snapshot.run.reviewers.map((r) => [r.id, r])),
  );

  const { counts } = $derived(snapshot.funnel);

  const stages = $derived([
    {
      key: "proposed" as const,
      label: "Proposed",
      subtitle: "Each reviewer reviewed independently",
      count: counts.proposed,
      width: "100%",
      peelAfter: "cross_checked" as FunnelStage | null,
    },
    {
      key: "cross_checked" as const,
      label: "Cross-checked",
      subtitle: "Agents validated or challenged",
      count: counts.cross_checked,
      width: "90%",
      peelAfter: "resolved" as FunnelStage | null,
    },
    {
      key: "resolved" as const,
      label: "Resolved",
      subtitle: "After arbiter — verdicts set",
      count: counts.resolved,
      width: "80%",
      peelAfter: null,
    },
    {
      key: "final" as const,
      label: "Final truth",
      subtitle: "Ships to your review",
      count: counts.final,
      width: "70%",
      peelAfter: null,
    },
  ]);

  function findingsInStage(stageKey: string): ArenaFinding[] {
    if (stageKey === "proposed") return snapshot.run.findings;
    if (stageKey === "final") {
      return snapshot.run.findings.filter(
        (f) => f.verdict !== "dropped" && f.verdict !== "pending",
      );
    }
    if (stageKey === "cross_checked") {
      return snapshot.run.findings.filter((f) => f.rounds.some((r) => r.n >= 2 && r.n < 200));
    }
    if (stageKey === "resolved") {
      return snapshot.run.findings.filter((f) => f.verdict !== "pending");
    }
    return snapshot.run.findings;
  }

  function peeledAt(stage: FunnelStage): ArenaFinding[] {
    return snapshot.run.findings.filter((f) => snapshot.funnel.exited_at[f.id] === stage);
  }
</script>

<div class="flex min-h-0 flex-1 flex-col items-center gap-0 overflow-y-auto px-4 py-4">
  {#each stages as stage, i}
    {#if stage.peelAfter}
      {@const peeled = peeledAt(stage.peelAfter)}
      {#if peeled.length > 0}
        <div class="mb-1 flex w-full max-w-[520px] items-start gap-2 pl-[12%]">
          <div class="mt-2 h-8 w-px shrink-0 bg-[var(--arena-border)]"></div>
          <div class="min-w-0 flex-1">
            <p class="text-[9px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]">
              {peeled.length} peeled off
            </p>
            <ul class="mt-1 space-y-0.5 opacity-50">
              {#each peeled.slice(0, 4) as f (f.id)}
                <li class="truncate text-[10px] text-[var(--arena-fg-muted)] line-through">
                  {f.title}
                </li>
              {/each}
            </ul>
          </div>
        </div>
      {/if}
    {/if}

    <section
      class="relative mx-auto w-full rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-2)] p-3 shadow-sm"
      style="max-width: {stage.width}"
    >
      {#if i > 0}
        <div
          class="absolute -top-3 left-1/2 h-3 w-px -translate-x-1/2 bg-[var(--arena-border)]"
          aria-hidden="true"
        ></div>
      {/if}
      <header class="mb-2">
        <div class="flex items-baseline justify-between gap-2">
          <h3 class="text-[11px] font-semibold uppercase tracking-wider text-[var(--arena-fg-subtle)]">
            {stage.label}
          </h3>
          <span class="mono text-[12px] font-bold text-[var(--arena-fg)]">{stage.count}</span>
        </div>
        <p class="text-[10px] text-[var(--arena-fg-faint)]">{stage.subtitle}</p>
      </header>

      {#if stage.key === "proposed"}
        <ul class="flex flex-wrap gap-1.5">
          {#each findingsInStage(stage.key).slice(0, 16) as f (f.id)}
            <li>
              <button
                type="button"
                class="arena-link-row inline-flex max-w-[220px] items-center gap-1 rounded-full border border-[var(--arena-border)] bg-[var(--arena-bg-0)] px-2 py-1 text-left text-[10px] {selectedId === f.id
                  ? 'arena-selected'
                  : ''}"
                onclick={() => onSelect(f.id)}
              >
                <span class="flex -space-x-0.5">
                  {#each f.raised_by.slice(0, 2) as rid}
                    {@const r = reviewerMap[rid]}
                    {#if r}
                      <span
                        class="inline-block h-2 w-2 rounded-full ring-1 ring-[var(--arena-bg-0)]"
                        style="background:{r.color}"
                        title={r.name}
                      ></span>
                    {/if}
                  {/each}
                </span>
                <span class="truncate text-[var(--arena-fg)]">{f.title}</span>
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <ul class="space-y-1">
          {#each findingsInStage(stage.key).slice(0, i === 0 ? 20 : 10) as f (f.id)}
            <li>
              <button
                type="button"
                class="arena-link-row w-full rounded px-2 py-1 text-left text-[11px] {selectedId === f.id
                  ? 'arena-selected'
                  : ''}"
                onclick={() => onSelect(f.id)}
              >
                <span class="text-[var(--arena-fg)]">{f.title}</span>
                <span class="mono ml-1 text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/each}
</div>
