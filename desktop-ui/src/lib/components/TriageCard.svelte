<script lang="ts">
  import type { TriageSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import Pill from "$lib/components/ui/Pill.svelte";
  import Disclosure from "$lib/components/ui/Disclosure.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import CardDeleteButton from "$lib/components/ui/CardDeleteButton.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { canRunAdvancedFollowUp, reviewFlowControls, reviewFlowStage } from "$lib/reviewFlow";
  import ReviewFlowProgress from "$lib/components/ReviewFlowProgress.svelte";
  import {
    filesPreview,
    followUpFor,
    previewLine,
    priorityDotClass,
    summaryPills,
    takeaways,
    triageFreshness,
    verdictLabel,
  } from "$lib/triageCard";
  import { tick } from "svelte";

  interface Props {
    triage: TriageSnapshot;
  }

  const { triage }: Props = $props();

  let open = $state(true);

  // The verdict, pills, impression and takeaways are the first read; the
  // reasoning rows stay collapsed until the reader wants them.
  let verdictOpen = $state(false);
  let filesOpen = $state(false);
  let advancedFollowUpPending = $state(false);

  const reviewScope = $derived(reviewScopeFromMode(app.snapshot?.mode));
  const label = $derived(verdictLabel(triage.verdict_primary));
  const pills = $derived(summaryPills(triage));

  const rows = $derived(takeaways(triage));
  const freshness = $derived(
    triageFreshness(triage, app.snapshot?.diff_stale?.built_from_oid),
  );
  const reviewFlow = $derived(app.snapshot?.review_flow ?? null);
  const flowControls = $derived(reviewFlowControls(reviewFlow));

  const verdictSummary = $derived.by(() => {
    const parts = [`Next: ${label}`];
    if (triage.confidence) parts.push(`(${triage.confidence} confidence)`);
    return parts.join(" ");
  });

  async function navigateToPath(path: string) {
    const snap = app.snapshot;
    if (!snap) return;
    const f = snap.files.find((file) => file.path === path);
    if (f) {
      await app.cmd("select_file", { idx: f.source_index });
      await tick();
    }
  }

  function runTriageAgain() {
    if (!reviewScope) return;
    void app.cmd("run_ai_triage_review", { scope: reviewScope });
  }

  async function runFollowUp() {
    if (!reviewScope || !canRunAdvancedFollowUp(reviewFlow, advancedFollowUpPending)) return;
    const next = followUpFor(triage, reviewScope);
    if (!next) return;
    advancedFollowUpPending = true;
    try {
      if (next.kind === "arena") {
        arena.openLauncher();
        return;
      }
      await app.cmd(next.command, next.args);
    } finally {
      advancedFollowUpPending = false;
    }
  }

  function startReviewFlow() {
    const snap = app.snapshot;
    if (!reviewScope || !snap?.review_bucket_er_dir || !flowControls.start) return;
    void app.cmd("start_review_flow", { scope: reviewScope, erDir: snap.review_bucket_er_dir });
  }

  function controlReviewFlow(command: "confirm_review_flow" | "decline_review_flow" | "cancel_review_flow") {
    const erDir = app.snapshot?.review_bucket_er_dir;
    if (!erDir) return;
    void app.cmd(command, { erDir });
  }

  function openFlowResults() {
    const snap = app.snapshot;
    const runId = snap?.review_flow?.arena_run_id;
    if (snap?.review_bucket_er_dir && runId) {
      void arena.openFlowOverlay(snap.review_bucket_er_dir, runId);
    }
  }

  const showFollowUp = $derived(
    reviewScope != null &&
      triage.verdict_primary !== "skip" &&
      triage.fresh,
  );

  async function discardTriage() {
    try {
      await app.cmd("delete_review_artifact", { kind: "triage" });
      app.showToast("success", "Triage discarded");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      app.showToast("error", msg);
    }
  }
</script>

<Card class="triage-card group flex min-w-0 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center justify-between gap-2">
    <button
      type="button"
      class="flex min-w-0 flex-1 items-center gap-2 text-left"
      aria-expanded={open}
      onclick={() => (open = !open)}
    >
      <SectionLabel>Triage</SectionLabel>
      <span
        class="font-mono text-[11px] {freshness.stale
          ? 'rounded border border-warning/30 bg-warning/10 px-1.5 text-warning'
          : 'text-add-fg'}"
        data-testid="triage-freshness"
      >{freshness.text}</span>
    </button>
    <CardDeleteButton label="Discard triage" onDelete={discardTriage} />
  </div>

  {#if open}
    <div class="mt-2.5 min-h-0 flex-1 overflow-x-hidden">
      {#if reviewFlow}
        <div class="mb-3">
          <ReviewFlowProgress flow={reviewFlow} />
        </div>
      {/if}
      <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1.5">
        <p class="m-0 flex items-baseline gap-1.5 text-[14px] text-fg">
          <span class="text-[11px] font-medium uppercase tracking-[0.06em] text-accent">Next</span>
          <b class="font-semibold">{label}</b>
          {#if triage.confidence}<span class="text-[12px] text-fg-3">· {triage.confidence} confidence</span>{/if}
        </p>
        {#if pills.length > 0}
          <div class="flex flex-wrap gap-1.5" data-testid="triage-pills">
            {#each pills as pill (pill.label)}
              <Pill tone={pill.tone} title={pill.title}>{pill.label}</Pill>
            {/each}
          </div>
        {/if}
      </div>

      {#if triage.domains.length > 0}
        <ul class="mt-1.5 flex flex-wrap gap-1" aria-label="Domains">
          {#each triage.domains as domain (domain)}
            <li class="rounded bg-hairline px-1 py-px font-mono text-[9px] text-fg-3">{domain}</li>
          {/each}
        </ul>
      {/if}

      {#if triage.first_impression}
        <MarkdownText
          text={triage.first_impression}
          className="mt-2.5 mb-2 max-w-[68ch] text-[14px] leading-relaxed text-fg"
        />
      {/if}

      <ul class="m-0 grid list-none p-0" aria-label="Takeaways">
        {#each rows as row (row.key)}
          <li class="grid grid-cols-1 gap-0.5 border-t border-hairline py-[7px] text-[13px] sm:grid-cols-[76px_minmax(0,1fr)] sm:gap-3">
            <span class="pt-0.5 text-[11px] text-fg-3">{row.label}</span>
            <span class="min-w-0 break-words {row.empty ? 'text-fg-3' : 'text-fg-2'}">
              {row.text}{#if row.code}{row.text ? " · " : ""}<code class="font-mono text-[11.5px] text-fg">{row.code}</code>{/if}
            </span>
          </li>
        {/each}
      </ul>

      <div class="mt-1 min-w-0">
        <Disclosure
          label="Verdict"
          preview={previewLine(triage.rationale) || verdictSummary}
          bind:open={verdictOpen}
        >
          <p class="text-[12px] font-medium text-fg">{verdictSummary}</p>
          {#if triage.verdict_primary === "expert" && triage.experts.length > 0}
            <p class="mt-0.5 text-[11px] text-muted">
              Recommended experts: {triage.experts.join(", ")}
            </p>
          {/if}
          {#if triage.rationale}
            <p class="mt-1 text-[12px] leading-relaxed text-fg-2">{triage.rationale}</p>
          {/if}
        </Disclosure>

        {#if triage.priority_files.length > 0}
          <Disclosure
            label="Priority files"
            badge={triage.priority_files.length}
            preview={filesPreview(triage.priority_files)}
            bind:open={filesOpen}
          >
            <ul class="space-y-0.5">
              {#each triage.priority_files as pf, i (i)}
                <li class="min-w-0">
                  <button
                    type="button"
                    class="flex w-full min-w-0 items-start gap-1.5 rounded px-1 py-0.5 text-left transition-colors hover:bg-bg"
                    title={pf.reason ? `${pf.path} · ${pf.reason}` : pf.path}
                    onclick={() => navigateToPath(pf.path)}
                  >
                    <span
                      class="mt-[5px] h-1.5 w-1.5 shrink-0 rounded-full {priorityDotClass(pf.risk)}"
                      aria-hidden="true"
                    ></span>
                    <span class="sr-only">{pf.risk} risk</span>
                    <span class="min-w-0 flex-1">
                      <span class="truncate-start block font-mono text-[11px] text-fg-2">
                        <span class="truncate-start-inner">{pf.path}</span>
                      </span>
                      {#if pf.reason}
                        <span class="block truncate text-[10px] text-muted">{pf.reason}</span>
                      {/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          </Disclosure>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 flex-wrap gap-2 pt-3">
      {#if reviewFlow && !flowControls.busy}
        <span class="self-center text-[10px] text-muted">{reviewFlowStage(reviewFlow)}</span>
      {/if}
      {#if reviewScope}
        {#if flowControls.confirm}
          {#if reviewFlow}<span class="self-center text-[10px] text-muted">~${reviewFlow.estimated_cost?.toFixed(2) ?? "?"} · ~{reviewFlow.estimated_latency_seconds ?? "?"}s</span>{/if}
          <Button size="sm" variant="primary" onclick={() => controlReviewFlow("confirm_review_flow")}>Confirm Deep</Button>
          <Button size="sm" variant="ghost" onclick={() => controlReviewFlow("decline_review_flow")}>Decline</Button>
        {:else if flowControls.busy}
          <Button size="sm" variant="ghost" disabled>{reviewFlow ? reviewFlowStage(reviewFlow) : "Review running"}</Button>
          {#if flowControls.cancel}<Button size="sm" variant="ghost" onclick={() => controlReviewFlow("cancel_review_flow")}>Cancel</Button>{/if}
        {:else if flowControls.openRunId}
          <Button size="sm" variant="secondary" onclick={openFlowResults}>{(reviewFlow?.awaiting_accept ?? 0) > 0 ? "Accept results" : "View results"}</Button>
        {:else}
          <Button size="sm" variant="primary" disabled={!flowControls.start} onclick={startReviewFlow}>Review</Button>
        {/if}
      {/if}
      {#if reviewScope && showFollowUp}
        <details class="relative">
          <summary class="cursor-pointer list-none rounded-md px-3 py-1.5 text-xs font-medium uppercase tracking-wider text-fg-3 hover:bg-hover">
            Advanced
          </summary>
          <div class="absolute bottom-full left-0 z-10 mb-1 rounded-md border border-border bg-bg p-2 shadow-lg">
            <Button size="sm" variant="secondary" disabled={!canRunAdvancedFollowUp(reviewFlow, advancedFollowUpPending)} onclick={() => void runFollowUp()}>
              Run {label}
            </Button>
          </div>
        </details>
      {/if}
      {#if reviewScope}
        <Button size="sm" variant="ghost" onclick={runTriageAgain}>
          Re-triage
        </Button>
      {/if}
    </div>
  {/if}
</Card>
