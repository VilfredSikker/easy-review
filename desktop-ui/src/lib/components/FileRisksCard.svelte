<script lang="ts">
  import type { FileRiskSnapshot } from "$lib/types";
  import { tick } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import { riskCounts, riskQueueRows, type RiskQueueSort } from "$lib/fileRiskQueue";
  import { riskDotClass } from "$lib/fileStatus";
  import { diffNav } from "$lib/stores/diffNav.svelte";

  interface Props {
    risks: FileRiskSnapshot[];
  }

  const { risks }: Props = $props();

  /// A walk-the-list surface, so the order is the reader's choice. Every signal
  /// here needs no AI: the verdict comes from the review, churn from the diff.
  const SORTS: { value: RiskQueueSort; label: string; hint: string }[] = [
    { value: "risk", label: "risk", hint: "Review severity, highest first" },
    { value: "churn", label: "churn", hint: "Added + deleted lines, most first" },
    { value: "status", label: "status", hint: "Added, deleted, renamed, then other files" },
    { value: "findings", label: "findings", hint: "Finding count, most first" },
    { value: "comments", label: "comments", hint: "Comments + questions, most first" },
    { value: "path", label: "path", hint: "File path, alphabetical" },
  ];

  let sort = $state<RiskQueueSort>("risk");

  const rows = $derived(riskQueueRows(risks, app.snapshot?.files ?? [], sort));
  const counts = $derived(riskCounts(rows));
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
  <div class="flex items-baseline justify-between gap-2 flex-wrap">
    <SectionLabel>File risks</SectionLabel>
    <div class="flex items-center gap-1.5 text-[10px] mono">
      {#if counts.high > 0}
        <span class="text-risk-high">{counts.high} high</span>
      {/if}
      {#if counts.med > 0}
        <span class="text-risk-med">{counts.med} med</span>
      {/if}
      {#if counts.low > 0}
        <span class="text-risk-low">{counts.low} low</span>
      {/if}
    </div>
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

  <ul class="mt-3 max-h-64 space-y-0.5 overflow-y-auto">
    {#each rows as row (row.path)}
      <li class="flex items-center gap-1">
        <button
          type="button"
          class="flex min-w-0 flex-1 items-center gap-2 rounded-md px-1 py-1 text-left hover:bg-bg disabled:cursor-not-allowed disabled:hover:bg-transparent"
          class:opacity-60={row.reviewed}
          disabled={!canJumpTo(row.path)}
          title={canJumpTo(row.path) ? row.riskReason || row.path : `${row.path} is not in the current diff`}
          aria-label="{row.risk} risk, {row.path}"
          onclick={() => jumpTo(row.path)}
        >
          <span
            class="h-1.5 w-1.5 shrink-0 rounded-full {riskDotClass(row.risk)}"
            aria-hidden="true"
          ></span>
          <span class="truncate-start min-w-0 flex-1 font-mono text-[11px] text-fg-2">
            <span class="truncate-start-inner">{row.path}</span>
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
    {/each}
  </ul>
</Card>
