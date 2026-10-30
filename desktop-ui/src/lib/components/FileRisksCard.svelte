<script lang="ts">
  import type { FileRiskSnapshot, RiskTier } from "$lib/types";
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import {
    ratedRows,
    riskCounts,
    riskQueueRows,
    riskWhy,
    type RiskQueueSort,
  } from "$lib/fileRiskQueue";
  import { baseName, shortFileName } from "$lib/shortFileName";
  import { diffNav } from "$lib/stores/diffNav.svelte";

  interface Props {
    risks: FileRiskSnapshot[];
  }

  const { risks }: Props = $props();

  /// A walk-the-list surface, so the order is the reader's choice. Every signal
  /// here needs no AI: the level comes from the risk rules, churn from the diff.
  const SORTS: { value: RiskQueueSort; label: string; hint: string }[] = [
    { value: "risk", label: "risk", hint: "Risk level, highest first" },
    { value: "churn", label: "churn", hint: "Added + deleted lines, most first" },
    { value: "status", label: "status", hint: "Added, deleted, renamed, then other files" },
    { value: "findings", label: "findings", hint: "Finding count, most first" },
    { value: "comments", label: "comments", hint: "Comments + questions, most first" },
    { value: "path", label: "path", hint: "File path, alphabetical" },
  ];

  const LEVEL_TEXT: Record<RiskTier, string> = {
    high: "text-risk-high",
    med: "text-risk-med",
    low: "text-risk-low",
    ai: "text-periwinkle",
  };
  const LEVEL_DOT: Record<RiskTier, string> = {
    high: "bg-risk-high",
    med: "bg-risk-med",
    low: "bg-risk-low",
    ai: "bg-periwinkle",
  };

  let sort = $state<RiskQueueSort>("risk");

  const allRows = $derived(riskQueueRows(risks, app.snapshot?.files ?? [], sort));
  const rows = $derived(ratedRows(allRows));
  const counts = $derived(riskCounts(allRows));
  const currentPaths = $derived(new Set((app.snapshot?.files ?? []).map((file) => file.path)));
  const activeSortHint = $derived(SORTS.find((option) => option.value === sort)?.hint ?? "");

  function canJumpTo(path: string): boolean {
    return currentPaths.has(path);
  }

  async function jumpTo(path: string) {
    const snap = app.snapshot;
    if (!snap) return;
    const f = snap.files.find((file) => file.path === path);
    if (f) {
      await app.cmd("select_file", { idx: f.source_index });
      await tick();
      await diffNav.scrollToFile(path);
    }
  }

  /// Reviewed rows dim rather than vanish, so the queue keeps its shape while
  /// you work down it.
  function toggleReviewed(path: string, reviewed: boolean) {
    void app.cmd(reviewed ? "unmark_reviewed" : "mark_reviewed", { path });
  }
</script>

<Card>
  <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
    <SectionLabel>File risks</SectionLabel>
    <div class="flex items-center gap-1.5 text-[11px] mono" data-testid="risk-counts">
      {#if counts.high > 0}<span class="text-risk-high">{counts.high} high</span>{/if}
      {#if counts.med > 0}<span class="text-risk-med">{counts.med} med</span>{/if}
      {#if counts.low > 0}<span class="text-risk-low">{counts.low} low</span>{/if}
      {#if counts.ai > 0}<span class="text-fg-3">{counts.ai} not rated</span>{/if}
    </div>
    <span class="text-[11.5px] text-fg-3">from your risk rules; the AI rates the rest</span>
  </div>

  <div
    class="mt-2 -mx-1 max-w-full overflow-x-auto"
    role="group"
    aria-label="Sort files by"
  >
    <div class="flex w-max min-w-full flex-nowrap items-center gap-1 px-1 text-[10px] mono">
      {#each SORTS as option (option.value)}
        <button
          type="button"
          title={option.hint}
          aria-pressed={sort === option.value}
          onclick={() => (sort = option.value)}
          class="shrink-0 whitespace-nowrap px-1.5 py-0.5 rounded {sort === option.value
            ? 'bg-hairline text-fg'
            : 'text-fg-3 hover:bg-hover'}"
        >{option.label}</button>
      {/each}
    </div>
  </div>
  <p class="mt-1 px-1 text-[9px] leading-relaxed text-fg-3">Sorted by {activeSortHint.toLowerCase()}.</p>

  <ul class="mt-3 max-h-80 space-y-0.5 overflow-y-auto">
    {#each rows as row (row.path)}
      {@const name = shortFileName(baseName(row.path))}
      {@const why = riskWhy(row)}
      <li class="flex items-center gap-1">
        <button
          type="button"
          class="grid min-w-0 flex-1 grid-cols-[58px_minmax(0,1fr)_auto] items-center gap-x-3 gap-y-0.5 rounded-md px-1.5 py-1 text-left text-[12.5px] hover:bg-bg disabled:cursor-not-allowed disabled:hover:bg-transparent"
          class:opacity-60={row.reviewed}
          disabled={!canJumpTo(row.path)}
          title={canJumpTo(row.path) ? row.path : `${row.path} is not in the current diff`}
          aria-label="{row.shown} risk, {row.path}"
          onclick={() => jumpTo(row.path)}
        >
          <span class="inline-flex items-center gap-1.5 text-[11px] font-medium {LEVEL_TEXT[row.shown]}">
            <i class="h-[7px] w-[7px] shrink-0 rounded-[2px] {LEVEL_DOT[row.shown]}"></i>
            {row.shown}
          </span>
          <span class="min-w-0">
            <span class="block truncate font-medium text-fg {row.reviewed ? 'line-through text-fg-3' : ''}">
              {name.main}{#if name.stamp}<span class="text-fg-3"> {name.stamp}</span>{/if}
            </span>
            {#if why.text}
              <span class="block truncate text-[11.5px] text-fg-2">
                {#if why.source === "ai"}<span class="text-periwinkle">✦ </span>{/if}{why.text}
              </span>
            {/if}
          </span>
          <span class="mono text-right text-[11px] whitespace-nowrap">
            {#if why.source === "ai"}<span class="mr-1.5 rounded-[3px] bg-periwinkle/14 px-1 text-[10px] font-semibold text-periwinkle">AI</span>{/if}
            <span class="text-add-fg">+{row.additions}</span>
            <span class="text-del-fg">−{row.deletions}</span>
          </span>
        </button>
        <button
          type="button"
          title={row.reviewed ? "Mark not reviewed" : "Mark reviewed"}
          aria-label="{row.reviewed ? 'Unmark' : 'Mark'} {row.path} reviewed"
          onclick={() => toggleReviewed(row.path, row.reviewed)}
          class="shrink-0 px-1 py-1 rounded text-[10px] {row.reviewed
            ? 'text-fg-3'
            : 'text-fg-3 hover:bg-hover'}"
        >{row.reviewed ? "✓" : "○"}</button>
      </li>
    {:else}
      <li class="px-1 py-2 text-[12px] text-fg-3">No file has a risk level yet.</li>
    {/each}
  </ul>
</Card>
