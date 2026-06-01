<script lang="ts">
  import type { ArenaFinding, ArenaRunSnapshot } from "$lib/types/arena";
  import { basename, voteGlyph } from "$lib/arena/display";
  interface Props {
    snapshot: ArenaRunSnapshot;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }

  const { snapshot, selectedId, onSelect }: Props = $props();

  let expandedCardIds = $state<Set<string>>(new Set());

  const reviewerMap = $derived(
    Object.fromEntries(snapshot.run.reviewers.map((r) => [r.id, r])),
  );
  const crossCheckRounds = $derived(
    Array.from({ length: Math.max(0, snapshot.run.config.rounds - 1) }, (_, i) => i + 2),
  );
  const showArbiter = $derived(
    snapshot.run.config.rounds >= 2 && snapshot.run.reviewers.length >= 2,
  );

  function arbiterBallot(f: ArenaFinding) {
    const r = f.rounds.find((x) => x.n === 255);
    return r?.log[0];
  }

  function ballotsForRound(f: ArenaFinding, n: number) {
    return f.rounds.find((r) => r.n === n)?.log ?? [];
  }

  function isCardExpanded(id: string): boolean {
    return expandedCardIds.has(id);
  }

  function onCardClick(id: string) {
    if (expandedCardIds.has(id)) {
      const next = new Set(expandedCardIds);
      next.delete(id);
      expandedCardIds = next;
      return;
    }
    expandedCardIds = new Set([...expandedCardIds, id]);
    onSelect(id);
  }

  $effect(() => {
    if (selectedId && !expandedCardIds.has(selectedId)) {
      expandedCardIds = new Set([...expandedCardIds, selectedId]);
    }
  });
</script>

<div class="flex min-h-0 flex-1 gap-2 overflow-x-auto p-3">
  <section
    class="flex min-w-[200px] flex-1 flex-col rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)]"
  >
    <header
      class="shrink-0 border-b border-[var(--arena-border)] px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-subtle)]"
    >
      R1 · Propose
    </header>
    <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
      {#each snapshot.run.findings as f (f.id)}
        {@const ballots = ballotsForRound(f, 1)}
        {@const expanded = isCardExpanded(f.id)}
        <button
          type="button"
          aria-expanded={expanded}
          class="arena-link-row relative w-full rounded-md px-2 py-2 text-left {selectedId === f.id ? 'arena-selected' : ''} {!expanded ? 'max-h-[7.25rem] overflow-hidden' : ''}"
          onclick={() => onCardClick(f.id)}
        >
          <p class="text-[11px] font-medium text-[var(--arena-fg)]">{f.title}</p>
          <p class="mono truncate text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</p>
          {#if ballots.length > 0}
            <ul class="mt-1 space-y-1">
              {#each ballots as b}
                {@const agent = reviewerMap[b.reviewer]}
                <li class="flex gap-1.5 text-[10px] text-[var(--arena-fg-muted)]">
                  <span class="mono mt-0.5 w-3 shrink-0 text-center">{voteGlyph(b.vote)}</span>
                  <div class="min-w-0 flex-1">
                    <span
                      style={agent ? `color:${agent.color}` : ""}
                      class="font-semibold"
                    >{agent?.name ?? b.reviewer}</span>
                    {#if b.note}
                      <p class="mt-0.5 leading-snug opacity-80 {expanded ? '' : 'line-clamp-2'}">{b.note}</p>
                    {/if}
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
          {#if !expanded && ballots.some((b) => (b.note?.length ?? 0) > 80)}
            <p class="mt-1 text-[9px] font-semibold text-[var(--arena-periwinkle)]">Show full · click</p>
            <div
              class="pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-gradient-to-t from-[var(--arena-bg-0)] to-transparent"
              aria-hidden="true"
            ></div>
          {/if}
        </button>
      {/each}
    </div>
  </section>

  {#each crossCheckRounds as roundNum}
    <section
      class="flex min-w-[200px] flex-1 flex-col rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)]"
    >
      <header
        class="shrink-0 border-b border-[var(--arena-border)] px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-subtle)]"
      >
        R{roundNum} · Cross-check
      </header>
      <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
        {#each snapshot.run.findings as f (f.id)}
          {@const ballots = ballotsForRound(f, roundNum)}
          {@const expanded = isCardExpanded(f.id)}
          {#if ballots.length > 0}
            <button
              type="button"
              aria-expanded={expanded}
              class="arena-link-row relative w-full rounded-md px-2 py-2 text-left {selectedId === f.id ? 'arena-selected' : ''} {!expanded ? 'max-h-[7.25rem] overflow-hidden' : ''}"
              onclick={() => onCardClick(f.id)}
            >
              <p class="text-[11px] font-medium text-[var(--arena-fg)]">{f.title}</p>
              <p class="mono truncate text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</p>
              <ul class="mt-1 space-y-1">
                {#each ballots as b}
                  {@const agent = reviewerMap[b.reviewer]}
                  <li class="flex gap-1.5 text-[10px] text-[var(--arena-fg-muted)]">
                    <span class="mono mt-0.5 w-3 shrink-0 text-center">{voteGlyph(b.vote)}</span>
                    <div class="min-w-0 flex-1">
                      <span
                        style={agent ? `color:${agent.color}` : ""}
                        class="font-semibold"
                      >{agent?.name ?? b.reviewer}</span>
                      {#if b.note}
                        <p class="mt-0.5 leading-snug opacity-80 {expanded ? '' : 'line-clamp-2'}">{b.note}</p>
                      {/if}
                    </div>
                  </li>
                {/each}
              </ul>
              {#if !expanded && ballots.some((b) => (b.note?.length ?? 0) > 80)}
                <p class="mt-1 text-[9px] font-semibold text-[var(--arena-periwinkle)]">Show full · click</p>
                <div
                  class="pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-gradient-to-t from-[var(--arena-bg-0)] to-transparent"
                  aria-hidden="true"
                ></div>
              {/if}
            </button>
          {/if}
        {/each}
      </div>
    </section>
  {/each}

  {#if showArbiter}
    <section
      class="flex min-w-[220px] flex-1 flex-col rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)]"
    >
      <header
        class="shrink-0 border-b border-[var(--arena-border)] px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-periwinkle)]"
      >
        Arbiter
        {#if snapshot.arbiter}
          <span class="ml-1 font-normal normal-case text-[var(--arena-fg-subtle)]"
            >{snapshot.arbiter.label}</span
          >
        {/if}
      </header>
      <div class="min-h-0 flex-1 space-y-1 overflow-y-auto p-2">
        {#each snapshot.run.findings as f (f.id)}
          {@const b = arbiterBallot(f)}
          {@const text = f.rationale || b?.note || "—"}
          {@const expanded = isCardExpanded(f.id)}
          <button
            type="button"
            aria-expanded={expanded}
            class="arena-link-row relative w-full rounded-md px-2 py-2 text-left {selectedId === f.id ? 'arena-selected' : ''} {!expanded && text.length > 80 ? 'max-h-[7.25rem] overflow-hidden' : ''}"
            onclick={() => onCardClick(f.id)}
          >
            <p class="text-[11px] font-medium text-[var(--arena-fg)]">{f.title}</p>
            <p class="mono truncate text-[9px] text-[var(--arena-fg-faint)]">{basename(f.file)}</p>
            <p class="mt-1 text-[10px] leading-snug text-[var(--arena-fg-muted)] {expanded ? '' : 'line-clamp-3'}">
              {text}
            </p>
            {#if !expanded && text.length > 80}
              <p class="mt-1 text-[9px] font-semibold text-[var(--arena-periwinkle)]">Show full · click</p>
              <div
                class="pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-gradient-to-t from-[var(--arena-bg-0)] to-transparent"
                aria-hidden="true"
              ></div>
            {/if}
          </button>
        {/each}
      </div>
    </section>
  {/if}
</div>
