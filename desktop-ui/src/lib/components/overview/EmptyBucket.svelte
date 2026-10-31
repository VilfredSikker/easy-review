<script lang="ts">
  import type { AppSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { resolveActivePrNumber } from "$lib/prUrl";
  import { sourceToggleState } from "$lib/sourceToggle";
  import { emptyBucketCopy } from "./emptyBucket";
  import { reviewFlowCommand } from "./overviewModel";
  import { reviewFlowControls } from "$lib/reviewFlow";

  interface Props {
    /** Defaults to the live snapshot; stories pass one. */
    snapshot?: AppSnapshot | null;
  }

  const { snapshot: snapshotOverride }: Props = $props();
  const snap = $derived(snapshotOverride !== undefined ? snapshotOverride : app.snapshot);
  const copy = $derived(emptyBucketCopy(snap));
  const scope = $derived(reviewScopeFromMode(snap?.mode));
  const prNumber = $derived(resolveActivePrNumber(snap ?? null));
  // The same review flow as the Overview's AI review button and ⌘K → a → r.
  const reviewCmd = $derived(reviewFlowCommand(snap ?? null, scope));
  const flowBusy = $derived(reviewFlowControls(snap?.review_flow).busy);
  const toggle = $derived(
    sourceToggleState({
      prNumber,
      tabKind: snap?.tabs.find((t) => t.is_active)?.kind,
      localBranchCheckedOut: snap?.local_branch_checked_out,
    }),
  );
  const canSwitch = $derived(copy?.otherView === "pr" ? toggle.prAvailable : toggle.localAvailable);

  function switchView() {
    if (!copy || !canSwitch) return;
    if (copy.otherView === "pr") void app.cmd("set_mode", { mode: "pr_diff", prNumber });
    else void app.cmd("set_mode", { mode: "branch" });
  }
</script>

{#if copy}
  <div
    class="grid gap-3 rounded-[10px] border border-line bg-surface p-[18px]"
    role="status"
    data-testid="overview-empty-bucket"
  >
    <div>
      <b class="font-semibold text-fg">{copy.title}</b>
      <p class="m-0 mt-1 max-w-[60ch] text-[13px] text-fg-2">{copy.body}</p>
    </div>
    <div class="flex flex-wrap gap-2">
      <button
        type="button"
        class="inline-flex h-7 items-center rounded-[7px] bg-accent px-2.5 text-[12.5px] font-medium text-on-accent hover:bg-accent-hover disabled:opacity-50"
        disabled={!scope}
        onclick={() => scope && void app.cmd("run_ai_triage_review", { scope })}
      >Triage branch</button>
      <button
        type="button"
        class="inline-flex h-7 items-center rounded-[7px] border border-border bg-surface px-2.5 text-[12.5px] font-medium text-fg hover:border-fg-3 disabled:opacity-50"
        disabled={reviewCmd?.command !== "start_review_flow"}
        title={flowBusy ? "A review is running or waiting for confirmation" : undefined}
        onclick={() => reviewCmd && void app.cmd(reviewCmd.command, reviewCmd.args)}
      >Run review</button>
      {#if canSwitch}
        <button
          type="button"
          class="inline-flex h-7 items-center rounded-[7px] px-2.5 text-[12.5px] font-medium text-fg-2 hover:bg-hover hover:text-fg"
          onclick={switchView}
        >{copy.switchLabel}</button>
      {/if}
    </div>
  </div>
{/if}
