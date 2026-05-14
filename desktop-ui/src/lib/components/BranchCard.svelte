<script lang="ts">
  import type { PrSnapshot } from "$lib/types";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";

  interface Props {
    branch: string;
    base: string;
    pr: PrSnapshot | null;
    reviewed_count: number;
    total_count: number;
    additions: number;
    deletions: number;
    checks_status: "success" | "pending" | "failure" | null;
    is_pr?: boolean;
    pr_number?: number | null;
    is_merged?: boolean;
  }

  const {
    branch,
    base,
    pr,
    reviewed_count,
    total_count,
    additions,
    deletions,
    checks_status,
    is_pr = false,
    pr_number = null,
    is_merged = false,
  }: Props = $props();
</script>

<Card>
  <div class="flex items-center justify-between mb-3">
    <SectionLabel>Branch</SectionLabel>
    <button aria-label="Pin review" title="Pin review" class="text-muted hover:text-fg-2">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 17v5M9 10.76V19l3 2 3-2v-8.24"/><path d="M3 7l9-5 9 5"/></svg>
    </button>
  </div>
  <div class="text-sm mono mb-3 truncate text-fg-2 flex items-center gap-1.5">
    <span class="truncate">{branch}</span>
    {#if total_count > 0}
      <span class="shrink-0 px-1.5 py-0.5 rounded-full text-[10px] font-sans bg-bg text-muted">
        {reviewed_count}/{total_count} reviewed
      </span>
    {/if}
    {#if is_pr}
      <span title={is_merged ? "Merged PR" : "Pull request"} class="inline-flex items-center gap-1 shrink-0 {is_merged ? 'text-purple-400' : 'text-accent'}">
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7"/><line x1="6" y1="9" x2="6" y2="21"/></svg>
        {#if pr_number !== null}
          <span class="text-[10px] font-mono">#{pr_number}</span>
        {/if}
      </span>
    {/if}
  </div>
  <div class="text-xs text-fg-3 mb-3 mono">{base} ← <span class="text-fg-2">{branch}</span></div>

  <div class="space-y-2 text-sm">
    <div class="flex items-center gap-2">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><polyline points="16 16 12 12 8 16"/><line x1="12" y1="12" x2="12" y2="21"/><path d="M20.39 18.39A5 5 0 0 0 18 9h-1.26A8 8 0 1 0 3 16.3"/></svg>
      <span class="text-fg-2">Changes</span>
      <span class="ml-auto mono text-xs"><span class="text-add-fg">+{additions.toLocaleString()}</span> <span class="text-del-fg">−{deletions.toLocaleString()}</span></span>
    </div>
    {#if pr}
      <div class="flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2"><circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 9v6a3 3 0 0 0 3 3h9"/></svg>
        <span class="text-fg-2">PR #{pr.number}</span>
        <span class="ml-auto text-muted text-xs">{pr.state}</span>
      </div>
    {/if}
    {#if checks_status === "success"}
      <div class="flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#7ee2a8" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
        <span class="text-fg-2">Checks successful</span>
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2" class="ml-auto"><polyline points="9 18 15 12 9 6"/></svg>
      </div>
    {:else if checks_status === "pending"}
      <div class="flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#fbbf24" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>
        <span class="text-fg-2">Checks pending</span>
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2" class="ml-auto"><polyline points="9 18 15 12 9 6"/></svg>
      </div>
    {:else if checks_status === "failure"}
      <div class="flex items-center gap-2">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#f4a3a3" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/></svg>
        <span class="text-fg-2">Checks failing</span>
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#5e5e5e" stroke-width="2" class="ml-auto"><polyline points="9 18 15 12 9 6"/></svg>
      </div>
    {/if}
  </div>
</Card>
