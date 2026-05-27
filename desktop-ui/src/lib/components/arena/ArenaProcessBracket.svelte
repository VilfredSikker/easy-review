<script lang="ts">
  import type { ArenaFinding, ArenaRunSnapshot } from "$lib/types/arena";
  import { basename, voteGlyph } from "$lib/arena/display";
  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  const reviewerMap = $derived(
    Object.fromEntries(snapshot.run.reviewers.map((r) => [r.id, r])),
  );
  const rounds = [1, 2, 3] as const;
  const roundNames: Record<number, string> = {
    1: "Propose",
    2: "Cross-check",
    3: "Resolve",
  };

  function ballotsForRound(f: ArenaFinding, n: number) {
    return f.rounds.find((r) => r.n === n)?.log ?? [];
  }
</script>

<div class="flex min-h-0 flex-1 gap-2 overflow-x-auto p-3">
  {#each rounds as roundNum}
    <section
      class="flex min-w-[200px] flex-1 flex-col rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)]"
    >
      <header
        class="shrink-0 border-b border-[var(--arena-border)] px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-subtle)]"
      >
        R{roundNum} · {roundNames[roundNum]}
      </header>
      <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
        {#each snapshot.run.findings as f (f.id)}
          {@const ballots = ballotsForRound(f, roundNum)}
          {#if ballots.length > 0 || roundNum === 1}
            <button
              type="button"
              class="arena-link-row w-full rounded-md px-2 py-2 text-left {selectedId === f.id ? 'arena-selected' : ''}"
              onclick={() => onSelect(f.id)}
            >
              <p class="text-[11px] font-medium text-[var(--arena-fg)]">{f.title}</p>
              <p class="mono truncate text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</p>
              {#if ballots.length > 0}
                <ul class="mt-1 space-y-0.5">
                  {#each ballots as b}
                    {@const agent = reviewerMap[b.reviewer]}
                    <li class="flex gap-1 text-[10px] text-[var(--arena-fg-muted)]">
                      <span class="mono w-3 shrink-0 text-center">{voteGlyph(b.vote)}</span>
                      <span style={agent ? `color:${agent.color}` : ""} class="shrink-0 font-semibold"
                        >{agent?.name ?? b.reviewer}</span
                      >
                      {#if b.note}
                        <span class="truncate opacity-80">{b.note}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {:else if roundNum > 1}
                <p class="mt-1 text-[9px] text-[var(--arena-fg-faint)]">—</p>
              {/if}
            </button>
          {/if}
        {/each}
      </div>
    </section>
  {/each}
</div>
