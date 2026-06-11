<script lang="ts">
  import type { AiProviderInfo } from "$lib/types";
  import type { ReviewerInfo } from "$lib/types";
  import { agentCatalogEntry } from "$lib/arena/agents";
  import { formatModelPricePer1k } from "$lib/arena/estimate";

  interface Props {
    agents: ReviewerInfo[];
    providers: AiProviderInfo[];
    /** agent_kind → Set of provider::model keys */
    selection: Record<string, Set<string>>;
    isSingleMode: boolean;
    onToggleAgent: (kind: string) => void;
    onToggleModel: (kind: string, modelKey: string) => void;
  }

  const {
    agents,
    providers,
    selection,
    isSingleMode,
    onToggleAgent,
    onToggleModel,
  }: Props = $props();

  function pairKey(p: string, m: string) {
    return `${p}::${m}`;
  }

  function agentSelected(kind: string): boolean {
    return (selection[kind]?.size ?? 0) > 0;
  }
</script>

<div class="grid gap-2 sm:grid-cols-2">
  {#each agents as a (a.kind)}
    {@const meta = agentCatalogEntry(a.kind, a.label, a.description)}
    {@const on = agentSelected(a.kind)}
    {@const models = selection[a.kind] ?? new Set()}
    <div
      class="rounded-lg border p-3 transition-colors
        {on ? 'border-[color-mix(in_srgb,var(--arena-periwinkle)_55%,transparent)] bg-[var(--arena-bg-2)]' : 'border-[var(--arena-border)] bg-[var(--arena-bg-0)]'}"
    >
      <button
        type="button"
        class="flex w-full items-start gap-2.5 text-left"
        onclick={() => onToggleAgent(a.kind)}
      >
        <span
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
          style="background: {on ? meta.color : 'var(--arena-bg-3)'}"
        >
          <span class="text-[14px] leading-none" aria-hidden="true">{meta.glyph}</span>
        </span>
        <span class="min-w-0 flex-1">
          <span class="text-[12px] font-semibold text-[var(--arena-fg)]">{a.label}</span>
          <span class="mt-0.5 block text-[10px] text-[var(--arena-fg-subtle)]">{a.description}</span>
        </span>
        <span
          class="mt-1 flex h-4 w-4 shrink-0 items-center justify-center rounded border
            {on ? 'border-transparent bg-[var(--arena-periwinkle)]' : 'border-[var(--arena-border-strong)]'}"
        >
          {#if on}<span class="text-[10px] font-bold text-[var(--arena-bg-0)]">✓</span>{/if}
        </span>
      </button>

      {#if on}
        <div class="mt-2 flex flex-wrap gap-1 border-t border-[var(--arena-border)] pt-2">
          {#each providers as p (p.id)}
            {#each p.models as m (m.id)}
              {@const key = pairKey(p.id, m.id)}
              {@const picked = models.has(key)}
              <button
                type="button"
                title={m.label}
                onclick={() => onToggleModel(a.kind, key)}
                class="rounded-md border px-2 py-0.5 text-[10px] font-medium transition-colors
                  {picked
                  ? 'border-[var(--arena-periwinkle)] bg-[var(--arena-bg-3)] text-[var(--arena-fg)]'
                  : 'border-[var(--arena-border)] text-[var(--arena-fg-muted)] hover:bg-[var(--arena-bg-2)]'}"
              >
                {m.label}
                <span class="ml-1 font-mono text-[9px] opacity-70">{formatModelPricePer1k(m)}</span>
              </button>
            {/each}
          {/each}
        </div>
        {#if isSingleMode && models.size > 1}
          <p class="mt-1 text-[9px] text-[var(--arena-warn)]">Single mode: one model only</p>
        {/if}
      {/if}
    </div>
  {/each}
</div>
