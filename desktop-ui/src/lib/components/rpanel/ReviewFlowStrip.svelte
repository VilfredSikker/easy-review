<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { reviewFlowControls, reviewFlowStage } from "$lib/reviewFlow";
  import ReviewFlowProgress from "$lib/components/ReviewFlowProgress.svelte";
  import Button from "$lib/components/ui/Button.svelte";

  // The one-click review (ADR 0044) as the Findings tab shows it: the start
  // button when nothing has run, the Deep confirmation, then progress. The
  // Triage card carries the same controls once a triage exists.

  const reviewScope = $derived(reviewScopeFromMode(app.snapshot?.mode));
  const flow = $derived(app.snapshot?.review_flow ?? null);
  const controls = $derived(reviewFlowControls(flow));

  function start() {
    const erDir = app.snapshot?.review_bucket_er_dir;
    if (!reviewScope || !erDir || !controls.start) return;
    void app.cmd("start_review_flow", { scope: reviewScope, erDir });
  }

  function control(command: "confirm_review_flow" | "decline_review_flow" | "cancel_review_flow") {
    const erDir = app.snapshot?.review_bucket_er_dir;
    if (erDir) void app.cmd(command, { erDir });
  }

  function openResults() {
    const erDir = app.snapshot?.review_bucket_er_dir;
    const runId = flow?.arena_run_id;
    if (erDir && runId) void arena.openFlowOverlay(erDir, runId);
  }
</script>

{#if reviewScope}
  <div class="grid gap-2" data-testid="review-flow-controls">
    <div class="flex flex-wrap items-center gap-2">
      <p class="min-w-0 flex-1 truncate text-[11.5px] text-fg-3">
        {flow ? reviewFlowStage(flow) : "Triage, review and verify this diff"}
      </p>
      {#if controls.confirm && flow}
        <span class="mono text-[10.5px] text-fg-3">~${flow.estimated_cost?.toFixed(2) ?? "?"} · ~{flow.estimated_latency_seconds ?? "?"}s</span>
        <Button size="sm" variant="primary" onclick={() => control("confirm_review_flow")}>Confirm Deep</Button>
        <Button size="sm" variant="ghost" onclick={() => control("decline_review_flow")}>Cancel</Button>
      {:else if controls.busy}
        {#if controls.cancel}<Button size="sm" variant="ghost" onclick={() => control("cancel_review_flow")}>Cancel</Button>{/if}
      {:else if controls.openRunId}
        <Button size="sm" variant="secondary" onclick={openResults}>
          {(flow?.awaiting_accept ?? 0) > 0 ? "Accept results" : "View results"}
        </Button>
      {:else if controls.start}
        <Button size="sm" variant="primary" onclick={start}>Run review</Button>
      {/if}
    </div>
    {#if flow}
      <ReviewFlowProgress {flow} />
    {/if}
  </div>
{/if}
