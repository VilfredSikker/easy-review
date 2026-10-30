<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { localBranchBannerCopy } from "$lib/shellBanners";
  import { activeDiffSource, sourceToggleState } from "$lib/sourceToggle";
  import { resolveActivePrNumber } from "$lib/prUrl";

  /**
   * Under the view bar on a local PR tab's local branch view, saying how the
   * worktree differs from the PR head. The engine sets `local_branch_divergence`
   * only for that view. A stale diff takes the slot instead: the two banners
   * share one row, and catching up with origin comes first.
   */
  const snapshot = $derived(app.snapshot);
  const divergence = $derived(snapshot?.local_branch_divergence ?? null);
  const onLocal = $derived(activeDiffSource(snapshot?.mode, snapshot?.tour?.scope) === "local");
  const message = $derived(
    divergence && onLocal && !snapshot?.diff_stale ? localBranchBannerCopy(divergence) : null,
  );

  const prNumber = $derived(resolveActivePrNumber(snapshot));
  const activeTab = $derived(snapshot?.tabs.find((t) => t.is_active));
  const prAvailable = $derived(
    sourceToggleState({
      prNumber,
      tabKind: activeTab?.kind,
      localBranchCheckedOut: snapshot?.local_branch_checked_out,
    }).prAvailable,
  );
</script>

{#if message}
  <div
    class="flex shrink-0 flex-wrap items-center gap-2.5 border-b border-line bg-[color-mix(in_srgb,var(--color-action)_8%,var(--color-bg))] px-3.5 py-2 text-[12.5px] text-fg-2"
    role="status"
    data-testid="local-branch-banner"
  >
    <span class="font-semibold text-action">Local branch</span>
    <span class="min-w-0 flex-1">{message}</span>
    {#if prAvailable}
      <button
        type="button"
        class="ml-auto inline-flex h-6 shrink-0 items-center rounded-[7px] border border-border bg-surface px-2.5 text-[12px] font-medium text-fg hover:border-[color-mix(in_srgb,var(--color-fg)_25%,transparent)]"
        onclick={() => void app.cmd("set_mode", { mode: "pr_diff", prNumber })}
      >
        Back to PR diff
      </button>
    {/if}
  </div>
{/if}
