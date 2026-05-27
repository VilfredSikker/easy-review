<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { basename } from "$lib/arena/display";

  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  const { counts } = $derived(snapshot.funnel);
  const stages = $derived([
    { key: "proposed", label: "Proposed", count: counts.proposed, width: "100%" },
    { key: "cross_checked", label: "Cross-checked", count: counts.cross_checked, width: "88%" },
    { key: "resolved", label: "Resolved", count: counts.resolved, width: "76%" },
    { key: "final", label: "Final truth", count: counts.final, width: "66%" },
  ]);

  function findingsInStage(stageKey: string) {
    if (stageKey === "proposed") return snapshot.run.findings;
    if (stageKey === "final") {
      return snapshot.run.findings.filter(
        (f) => f.verdict !== "dropped" && f.verdict !== "pending",
      );
    }
    return snapshot.run.findings.filter((f) => {
      const exited = snapshot.funnel.exited_at[f.id];
      if (stageKey === "cross_checked") return f.rounds.some((r) => r.n >= 2);
      if (stageKey === "resolved") return f.rounds.some((r) => r.n >= 3) || exited;
      return true;
    });
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
  {#each stages as stage, i}
    <section
      class="mx-auto rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-2)] p-3"
      style="width: {stage.width}"
    >
      <header class="mb-2 flex items-baseline justify-between gap-2">
        <h3 class="text-[11px] font-semibold uppercase tracking-wider text-[var(--arena-fg-subtle)]">
          {stage.label}
        </h3>
        <span class="mono text-[12px] font-bold text-[var(--arena-fg)]">{stage.count}</span>
      </header>
      <ul class="space-y-1">
        {#each findingsInStage(stage.key).slice(0, i === 0 ? 20 : 12) as f (f.id)}
          <li>
            <button
              type="button"
              class="arena-link-row w-full rounded px-2 py-1 text-left text-[11px] {selectedId === f.id ? 'arena-selected' : ''}"
              onclick={() => onSelect(f.id)}
            >
              <span class="text-[var(--arena-fg)]">{f.title}</span>
              <span class="mono ml-1 text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</div>
