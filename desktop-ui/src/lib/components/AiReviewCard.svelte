<script lang="ts">
  import type { AiSnapshot, FlatFinding } from "$lib/types";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  let summaryOpen = $state(false);
  let expandedFindings = $state(new Set<string>());

  function toggleFinding(id: string) {
    const next = new Set(expandedFindings);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    expandedFindings = next;
  }

  function severityLabel(s: FlatFinding["severity"]): string {
    return s === "high" ? "H" : s === "med" ? "M" : "L";
  }

  function severityClass(s: FlatFinding["severity"]): string {
    return s === "high"
      ? "text-risk-high"
      : s === "med"
        ? "text-risk-med"
        : "text-risk-low";
  }

  function basename(path: string): string {
    return path.split("/").pop() ?? path;
  }
</script>

<div class="px-3 py-2.5 border-b border-ink-500/40">
  <div class="flex items-center justify-between mb-2">
    <span class="text-[10px] font-medium uppercase tracking-wider text-ink-400">AI Review</span>
    {#if !ai.fresh}
      <span class="text-[10px] px-1 py-0.5 rounded bg-amber-500/15 text-amber-400">stale</span>
    {/if}
  </div>

  <div class="flex items-center gap-3 mb-2">
    <span class="text-xs font-mono text-risk-high">{ai.high}<span class="text-ink-500 ml-0.5">H</span></span>
    <span class="text-xs font-mono text-risk-med">{ai.med}<span class="text-ink-500 ml-0.5">M</span></span>
    <span class="text-xs font-mono text-risk-low">{ai.low}<span class="text-ink-500 ml-0.5">L</span></span>
  </div>

  {#if ai.summary_markdown}
    <button
      class="flex items-center gap-1 text-[10px] text-ink-400 hover:text-ink-200 transition-colors mb-1.5 w-full text-left"
      onclick={() => (summaryOpen = !summaryOpen)}
    >
      <span class="font-mono">{summaryOpen ? "▼" : "▶"}</span>
      <span>Summary</span>
    </button>
    {#if summaryOpen}
      <div class="text-[11px] text-ink-300 whitespace-pre-wrap mb-2 leading-relaxed">
        {ai.summary_markdown}
      </div>
    {/if}
  {/if}

  {#if ai.findings.length > 0}
    <div class="flex flex-col gap-0.5">
      {#each ai.findings as finding (finding.id)}
        <div class="rounded overflow-hidden">
          <button
            class="w-full flex items-center gap-1.5 py-1 px-1 hover:bg-ink-800 transition-colors text-left"
            onclick={() => toggleFinding(finding.id)}
          >
            <span class="text-[10px] font-mono font-bold {severityClass(finding.severity)} shrink-0">
              ● {severityLabel(finding.severity)}
            </span>
            <span class="text-[10px] text-ink-500 shrink-0">{basename(finding.file)}</span>
            {#if finding.line !== null}
              <span class="text-[10px] text-ink-600 shrink-0">:{finding.line}</span>
            {/if}
            <span
              class="text-[10px] text-ink-200 truncate flex-1"
              title={finding.title}
            >{finding.title}</span>
            <span class="text-ink-600 text-[8px] shrink-0">{expandedFindings.has(finding.id) ? "▲" : "▼"}</span>
          </button>
          {#if expandedFindings.has(finding.id)}
            <div class="px-2 pb-1.5 text-[11px] text-ink-400 whitespace-pre-wrap leading-relaxed bg-ink-800/50">
              {finding.message_markdown}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {:else}
    <div class="text-[11px] text-ink-500">No findings</div>
  {/if}
</div>
