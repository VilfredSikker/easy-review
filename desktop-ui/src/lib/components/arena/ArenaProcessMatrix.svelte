<script lang="ts">
  import type { ArenaRunSnapshot } from "$lib/types/arena";
  import { basename, verdictLabel, verdictPillClass } from "$lib/arena/display";
  import { voteCellClass } from "$lib/arena/voteCell";
  import ArenaVoteIcon from "$lib/components/arena/ArenaVoteIcon.svelte";

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
        {#if snapshot.arbiter && snapshot.run.config.rounds >= 2}
          <th class="px-2 py-2 font-semibold text-[var(--arena-periwinkle)]">Arbiter</th>
        {/if}
        <th class="px-2 py-2 font-semibold">Verdict</th>
      </tr>
    </thead>
    <tbody>
      {#each snapshot.matrix as row}
        {@const f = snapshot.run.findings.find((x) => x.id === row.finding_id)}
        {#if f}
          <tr
            class="arena-link-row cursor-pointer border-b border-[var(--arena-border)] {row.verdict === 'dropped' ? 'opacity-55' : ''} {selectedId === f.id ? 'arena-selected' : ''}"
            onclick={() => onSelect(f.id)}
          >
            <td class="sticky left-0 z-10 max-w-[200px] bg-[var(--arena-bg-0)] px-2 py-2">
              <p class="truncate font-medium text-[var(--arena-fg)]">{f.title}</p>
              <p class="mono truncate text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</p>
            </td>
            {#each reviewers as r}
              {@const vote = row.latest_vote[r.id]}
              <td class="px-2 py-2 text-center {voteCellClass(vote)}">
                {#if vote}
                  <ArenaVoteIcon {vote} size={15} class="mx-auto" />
                {:else}
                  <span class="text-[var(--arena-fg-faint)]">—</span>
                {/if}
              </td>
            {/each}
            {#if snapshot.arbiter && snapshot.run.config.rounds >= 2}
              <td class="max-w-[140px] px-2 py-2 text-[10px] text-[var(--arena-fg-muted)]" title={row.arbiter_note}>
                {#if row.arbiter_vote}
                  <ArenaVoteIcon vote={row.arbiter_vote} size={15} class="mx-auto" />
                {/if}
                <span class="mt-0.5 whitespace-pre-wrap break-words">{row.arbiter_note || "—"}</span>
              </td>
            {/if}
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
