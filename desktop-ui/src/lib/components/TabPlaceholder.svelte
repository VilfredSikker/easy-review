<script lang="ts">
  import type { TabSummary } from "$lib/types";
  import { tabPlaceholderRepo } from "$lib/tabPlaceholder";

  /**
   * Stands in for a tab whose review is still loading: the engine is building
   * its diff and no cached snapshot can paint it yet. It names the tab being
   * opened, so the reader is not left looking at the previous tab's review
   * under a spinner and wondering which one they are on.
   */
  interface Props {
    tab: TabSummary;
    /** What the slow command is doing ("Switching tab..."). */
    status?: string | null;
  }

  const { tab, status = null }: Props = $props();

  const isPr = $derived(tab.kind === "remote_pr" || tab.pr_number != null);
  const repo = $derived(tabPlaceholderRepo(tab));
</script>

<div class="flex-1 min-w-0 min-h-0 overflow-auto bg-bg" data-testid="tab-placeholder" aria-busy="true">
  <div class="flex flex-wrap items-center gap-2.5 px-5 py-3.5 border-b border-line text-[13px]">
    <span class="grid place-items-center w-4 h-4 text-fg-3 shrink-0" aria-hidden="true">
      {#if isPr}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="18" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><path d="M6 3v12M18 9a9 9 0 0 1-9 9" /></svg>
      {:else}
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="6" cy="6" r="3" /><circle cx="6" cy="18" r="3" /><circle cx="18" cy="8" r="3" /><path d="M6 9v6M18 11c0 4-6 3-9 6" /></svg>
      {/if}
    </span>
    <b class="font-semibold text-fg min-w-0 truncate">{tab.label}</b>
    {#if repo}<span class="text-muted">{repo}</span>{/if}
    {#if tab.stale}
      <span class="inline-flex h-[22px] items-center rounded-md border border-[color-mix(in_srgb,var(--color-warning)_40%,transparent)] bg-[color-mix(in_srgb,var(--color-warning)_14%,transparent)] px-2 text-[10.5px] font-semibold uppercase tracking-[0.05em] text-warning">Stale</span>
    {/if}
  </div>
  <div class="mx-auto my-[60px] max-w-[460px] px-4">
    <div class="flex items-center gap-2.5 text-fg">
      <span class="h-4 w-4 shrink-0 rounded-full border-2 border-ink-600 border-t-accent motion-safe:animate-spin" aria-hidden="true"></span>
      <b class="font-semibold">{status ?? "Opening review…"}</b>
    </div>
    <p class="mt-2 text-[13px] leading-relaxed text-fg-3">
      Each tab is its own review target with its own diff, selection and artifacts. This one keeps its scroll,
      filters and drafts once it has loaded.
    </p>
  </div>
</div>
