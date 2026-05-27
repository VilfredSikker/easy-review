<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { basename, verdictLabel, verdictPillClass, voteGlyph } from "$lib/arena/display";

  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  const reviewers = $derived(snapshot.run.reviewers);
</script>

<div class="min-h-0 flex-1 overflow-auto p-3">
  <table class="w-full min-w-[640px] border-collapse text-left text-[11px]">
    <thead>
      <tr class="border-b border-[var(--arena-border)] text-[10px] uppercase tracking-wider text-[var(--arena-fg-faint)]">
        <th class="sticky left-0 z-10 bg-[var(--arena-bg-0)] px-2 py-2 font-semibold">Finding</th>
        {#each reviewers as r}
          <th class="px-2 py-2 font-semibold" style="color:{r.color}">{r.name}</th>
        {/each}
        <th class="px-2 py-2 font-semibold">Verdict</th>
      </tr>
    </thead>
    <tbody>
      {#each snapshot.matrix as row}
        {@const f = snapshot.run.findings.find((x) => x.id === row.finding_id)}
        {#if f}
          <tr
            class="arena-link-row cursor-pointer border-b border-[var(--arena-border)] {selectedId === f.id ? 'arena-selected' : ''}"
            onclick={() => onSelect(f.id)}
          >
            <td class="sticky left-0 z-10 max-w-[200px] bg-[var(--arena-bg-0)] px-2 py-2">
              <p class="truncate font-medium text-[var(--arena-fg)]">{f.title}</p>
              <p class="mono truncate text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</p>
            </td>
            {#each reviewers as r}
              {@const vote = row.latest_vote[r.id]}
              <td class="mono px-2 py-2 text-center text-[var(--arena-fg-muted)]">
                {vote ? voteGlyph(vote) : "—"}
              </td>
            {/each}
            <td class="px-2 py-2">
              <span class={verdictPillClass(row.verdict)}>{verdictLabel(row.verdict)}</span>
              <span class="mono ml-1 text-[10px] text-[var(--arena-fg-subtle)]"
                >{Math.round(row.confidence * 100)}%</span
              >
            </td>
          </tr>
        {/if}
      {/each}
    </tbody>
  </table>
</div>
