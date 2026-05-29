<script lang="ts">
  import type { TriageSnapshot } from "$lib/types";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";

  interface Props {
    triage: TriageSnapshot;
  }

  const { triage }: Props = $props();

  const verdictLabel = $derived.by(() => {
    switch (triage.verdict) {
      case "skip":
        return "Skip — low risk";
      case "review":
        return "Review recommended";
      case "deep_review":
        return "Deep review recommended";
      default:
        return triage.verdict;
    }
  });

  const verdictClass = $derived.by(() => {
    switch (triage.verdict) {
      case "skip":
        return "bg-add-fg/15 text-add-fg border-add-fg/30";
      case "review":
        return "bg-accent/15 text-accent border-accent/30";
      case "deep_review":
        return "bg-ai/15 text-ai border-ai/30";
      default:
        return "bg-hairline text-fg-2 border-border";
    }
  });

  const recommendationLine = $derived.by(() => {
    switch (triage.recommended_review) {
      case "expert":
      case "general":
        return "Run full review recommended";
      case "none":
        return "Optional glance";
      default:
        return null;
    }
  });

  const visibleSmells = $derived(triage.smells.slice(0, 5));
</script>

<Card>
  <div class="flex items-center justify-between gap-2 mb-2">
    <SectionLabel>Preemptive scan</SectionLabel>
    <span class="text-[10px] mono px-2 py-0.5 rounded border {verdictClass}">
      {verdictLabel}
    </span>
  </div>

  {#if triage.outdated}
    <p class="text-[10px] text-muted mb-2 italic">Scan may be outdated — PR diff changed since triage ran.</p>
  {/if}

  {#if triage.summary.trim()}
    <MarkdownText text={triage.summary} className="text-sm text-fg-2 leading-relaxed mb-3" />
  {/if}

  {#if visibleSmells.length > 0}
    <div class="mb-3 space-y-1.5">
      <div class="text-[10px] uppercase tracking-wider text-fg-3">Smells</div>
      {#each visibleSmells as smell (smell.text + smell.category)}
        <div class="flex items-start gap-2 text-[11px] text-fg-2">
          <span class="mono text-[10px] text-muted shrink-0 uppercase">{smell.severity}</span>
          <span class="min-w-0">{smell.text}</span>
        </div>
      {/each}
    </div>
  {/if}

  {#if triage.recommended_experts.length > 0}
    <div class="mb-3">
      <div class="text-[10px] uppercase tracking-wider text-fg-3 mb-1.5">Suggested experts</div>
      <div class="flex flex-wrap gap-1.5">
        {#each triage.recommended_experts as expert (expert)}
          <span class="text-[10px] mono px-2 py-0.5 rounded-full bg-hairline text-fg-2 border border-border">
            {expert}
          </span>
        {/each}
      </div>
    </div>
  {/if}

  {#if recommendationLine}
    <p class="text-[11px] text-fg-3">{recommendationLine}</p>
  {/if}
</Card>
