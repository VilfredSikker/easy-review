<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { navigateToFinding } from "$lib/dom";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  let open = $state(false);
  let summaryOpen = $state(false);
  let staleHelpOpen = $state(false);
  let filter = $state<"all" | "high" | "med" | "low">("all");

  function basename(p: string): string {
    const i = p.lastIndexOf("/");
    return i === -1 ? p : p.slice(i + 1);
  }

  function jumpTo(finding: (typeof filtered)[0]) {
    navigateToFinding(finding);
  }

  const isEmpty = $derived(
    ai.findings.length === 0 && ai.high + ai.med + ai.low === 0
  );

  const summary = $derived(
    isEmpty
      ? "No findings written. Inspect the `.er/` folder to see raw review output, or re-run the review skill."
      : (ai.summary_markdown ??
          `${ai.high + ai.med + ai.low} findings across ${new Set(ai.findings.map((f) => f.file)).size} files.`)
  );
  const staleReason = $derived(ai.stale_reason ?? "Review artifacts are stale.");

  const filtered = $derived(
    ai.findings.filter((f) => filter === "all" || f.severity === filter)
  );

  function revealErFolder() {
    invoke("reveal_er_folder").catch(() => {});
  }
</script>

<Card>
  <div class="flex items-center justify-between mb-2">
    <SectionLabel>AI Review</SectionLabel>
    {#if ai.fresh}
      <span class="text-[10px] mono text-add-fg">fresh</span>
    {:else}
      <div class="flex items-center gap-1">
        <span class="text-[10px] mono text-ai">stale</span>
        <button
          type="button"
          class="text-[10px] mono text-ai hover:text-fg-2"
          title={staleReason}
          aria-label={staleReason}
          aria-expanded={staleHelpOpen}
          onclick={() => staleHelpOpen = !staleHelpOpen}
        >?</button>
      </div>
    {/if}
  </div>
  {#if !ai.fresh && staleHelpOpen}
    <div class="mb-2 rounded border border-hairline bg-bg px-2 py-1.5 text-[11px] text-fg-2">
      {staleReason}
    </div>
  {/if}
  {#if summaryOpen || isEmpty}
    <div class="summary-expanded mb-3">
      <MarkdownText text={summary} className="text-sm text-fg-2 leading-relaxed" />
    </div>
  {:else}
    <div class="summary-preview mb-3 text-sm text-fg-2 leading-relaxed">
      <MarkdownText text={summary} />
    </div>
  {/if}

  {#if !isEmpty}
    <Button
      variant="secondary"
      onclick={() => summaryOpen = !summaryOpen}
      class="w-full mb-3"
    >
      {summaryOpen ? "Hide summary" : "Show summary"}
    </Button>
  {/if}

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
        <div class="relative group">
          <button
            onclick={() => jumpTo(finding)}
            class="w-full text-left p-2 pr-6 rounded-md hover:bg-bg border border-transparent hover:border-border block"
          >
            <div class="flex items-start gap-2">
              <span class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0 {dotClass}"></span>
              <div class="flex-1 min-w-0">
                <div class="text-[11px] font-mono text-muted mb-0.5">{basename(finding.file)}{finding.line !== null ? `:${finding.line}` : ""}</div>
                <div class="text-[13px] text-fg-2 leading-snug">{finding.title}</div>
              </div>
            </div>
          </button>
          <button
            type="button"
            onclick={() => app.cmd("dismiss_finding", { findingId: finding.id })}
            title="Dismiss finding"
            class="absolute top-1.5 right-1 p-0.5 rounded opacity-0 group-hover:opacity-100 transition hover:bg-del-bg text-muted hover:text-del-fg"
          >
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>
          </button>
        </div>
      {/each}
    </div>
  {/if}

  {#if !isEmpty}
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
  {/if}

  <button
    onclick={revealErFolder}
    class="w-full mt-2 flex items-center justify-center gap-2 text-[11px] mono text-fg-3 hover:text-fg py-1.5 rounded hover:bg-bg border border-transparent hover:border-border"
    title="Open the review files folder in your file manager"
  >
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z"/>
    </svg>
    <span>Reveal review files</span>
  </button>
</Card>

<style>
  .summary-preview {
    display: -webkit-box;
    overflow: hidden;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .summary-expanded {
    max-height: 20rem;
    overflow-y: auto;
  }
</style>
