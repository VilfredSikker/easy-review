<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import { jumpTo as scrollFlash } from "$lib/dom";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  let open = $state(false);
  let filter = $state<"all" | "high" | "med" | "low">("all");

  function basename(p: string): string {
    const i = p.lastIndexOf("/");
    return i === -1 ? p : p.slice(i + 1);
  }

  function jumpTo(findingId: string) {
    scrollFlash(`finding-${findingId}`);
  }

  const summary = $derived(
    ai.summary_markdown ??
      `${ai.high + ai.med + ai.low} findings across ${new Set(ai.findings.map((f) => f.file)).size} files.`
  );

  const filtered = $derived(
    ai.findings.filter((f) => filter === "all" || f.severity === filter)
  );
</script>

<Card>
  <div class="flex items-center justify-between mb-2">
    <SectionLabel>AI Review</SectionLabel>
    {#if ai.fresh}
      <span class="text-[10px] mono text-add-fg">fresh</span>
    {:else}
      <span class="text-[10px] mono text-ai">stale</span>
    {/if}
  </div>
  <div class="text-sm text-fg-2 leading-relaxed mb-3 whitespace-pre-wrap">{summary}</div>

  <div class="grid grid-cols-3 gap-2 mb-3">
    <button
      onclick={() => { open = true; filter = "high"; }}
      class="rounded-md bg-bg border px-2 py-1.5 text-left hover:border-risk-high {filter === 'high' && open ? 'border-risk-high' : 'border-border'}"
    >
      <div class="flex items-center gap-1.5 text-[10px] text-risk-high uppercase tracking-wider"><span class="w-1.5 h-1.5 rounded-full bg-risk-high"></span>High</div>
      <div class="text-lg font-semibold mono">{ai.high}</div>
    </button>
    <button
      onclick={() => { open = true; filter = "med"; }}
      class="rounded-md bg-bg border px-2 py-1.5 text-left hover:border-risk-med {filter === 'med' && open ? 'border-risk-med' : 'border-border'}"
    >
      <div class="flex items-center gap-1.5 text-[10px] text-risk-med uppercase tracking-wider"><span class="w-1.5 h-1.5 rounded-full bg-risk-med"></span>Med</div>
      <div class="text-lg font-semibold mono">{ai.med}</div>
    </button>
    <button
      onclick={() => { open = true; filter = "low"; }}
      class="rounded-md bg-bg border px-2 py-1.5 text-left hover:border-risk-low {filter === 'low' && open ? 'border-risk-low' : 'border-border'}"
    >
      <div class="flex items-center gap-1.5 text-[10px] text-risk-low uppercase tracking-wider"><span class="w-1.5 h-1.5 rounded-full bg-risk-low"></span>Low</div>
      <div class="text-lg font-semibold mono">{ai.low}</div>
    </button>
  </div>

  {#if open}
    <div class="mt-4 pt-3 border-t border-hairline space-y-1.5 mb-3">
      <div class="flex items-center gap-1.5 mb-2 text-[10px] mono">
        <button onclick={() => filter = "all"} class="px-2 py-0.5 rounded {filter === 'all' ? 'bg-hairline text-fg' : 'text-fg-3 hover:bg-hover'}">all</button>
        <button onclick={() => filter = "high"} class="px-2 py-0.5 rounded flex items-center gap-1 {filter === 'high' ? 'bg-hairline text-risk-high' : 'text-fg-3 hover:bg-hover'}"><span class="w-1.5 h-1.5 rounded-full bg-risk-high"></span>high</button>
        <button onclick={() => filter = "med"} class="px-2 py-0.5 rounded flex items-center gap-1 {filter === 'med' ? 'bg-hairline text-risk-med' : 'text-fg-3 hover:bg-hover'}"><span class="w-1.5 h-1.5 rounded-full bg-risk-med"></span>med</button>
        <button onclick={() => filter = "low"} class="px-2 py-0.5 rounded flex items-center gap-1 {filter === 'low' ? 'bg-hairline text-risk-low' : 'text-fg-3 hover:bg-hover'}"><span class="w-1.5 h-1.5 rounded-full bg-risk-low"></span>low</button>
      </div>

      {#each filtered as finding (finding.id)}
        {@const dotClass = finding.severity === "high" ? "bg-risk-high" : finding.severity === "med" ? "bg-risk-med" : "bg-risk-low"}
        <button
          onclick={() => jumpTo(finding.id)}
          class="w-full text-left p-2 rounded-md hover:bg-bg border border-transparent hover:border-border block"
        >
          <div class="flex items-start gap-2">
            <span class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0 {dotClass}"></span>
            <div class="flex-1 min-w-0">
              <div class="text-[11px] font-mono text-muted mb-0.5">{basename(finding.file)}{finding.line !== null ? `:${finding.line}` : ""}</div>
              <div class="text-[13px] text-fg-2 leading-snug">{finding.title}</div>
            </div>
          </div>
        </button>
      {/each}
    </div>
  {/if}

  <Button
    variant="secondary"
    onclick={() => open = !open}
    class="w-full flex items-center justify-center gap-2 normal-case"
  >
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="transition-transform {open ? 'rotate-180' : ''}">
      <polyline points="6 9 12 15 18 9"/>
    </svg>
    <span>{open ? "Hide findings" : "View findings"}</span>
  </Button>
</Card>
