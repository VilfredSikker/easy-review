<script lang="ts">
  import { tick } from "svelte";
  import type { AiSnapshot, FlatFinding } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { reviewPage } from "$lib/stores/reviewPage.svelte";
  import { findingsVisibility } from "$lib/stores/findingsVisibility.svelte";
  import { navigateToFinding } from "$lib/dom";
  import { activeDiffSource } from "$lib/sourceToggle";
  import { baseName } from "$lib/shortFileName";
  import { shortAge } from "$lib/relativeTime";
  import { findingAgentLabels } from "$lib/aiReviewAgents";
  import { findingLocation, SEVERITIES } from "$lib/components/rpanel/findingsPanel";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import PromoteModal from "$lib/components/PromoteModal.svelte";
  import {
    canPromote,
    confidenceWord,
    lensCaption,
    overviewFindingsView,
    promoteBody,
  } from "./findingsList";

  interface Props {
    /** Defaults to the live snapshot's review; stories pass one. */
    ai?: AiSnapshot | null;
    /** Defaults to whether the PR diff view is on screen. */
    onPrView?: boolean;
  }

  const { ai: aiOverride, onPrView: prViewOverride }: Props = $props();

  const ai = $derived(aiOverride !== undefined ? aiOverride : (app.snapshot?.ai ?? null));
  const onPrView = $derived(
    prViewOverride ?? activeDiffSource(app.snapshot?.mode, app.snapshot?.tour?.scope) === "pr",
  );
  // The Gate (CONTEXT.md) the right panel and the diff use, so the three lists
  // never disagree about which findings exist.
  const minTrust = $derived(ai ? findingsVisibility.minTrust(ai.min_trust_default) : "informational");
  const view = $derived(overviewFindingsView(ai, minTrust));
  const caption = $derived(lensCaption(view.lenses, view.reviewerCount));
  const hasReview = $derived(
    !!ai && (ai.has_review_json || view.openCount > 0 || view.resolvedCount > 0),
  );
  const age = $derived(ai?.review_created_at ? shortAge(ai.review_created_at) : "");

  const SEV_TEXT = { high: "text-risk-high", med: "text-risk-med", low: "text-risk-low" } as const;
  const SEV_BG = { high: "bg-risk-high", med: "bg-risk-med", low: "bg-risk-low" } as const;

  const fileLine = (f: FlatFinding) => (f.line === null ? f.file : `${f.file}:${f.line}`);

  async function open(finding: FlatFinding) {
    reviewPage.show("diff");
    await tick();
    await navigateToFinding(finding);
  }

  // Resolve keeps the finding, dimmed, where Dismiss removes it (ADR 0047).
  function toggleResolved(finding: FlatFinding) {
    const command = finding.resolved ? "unresolve_finding" : "resolve_finding";
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked(command);
    void app.cmd(command, { findingId: finding.id });
  }

  let promoting = $state<FlatFinding | null>(null);
  const promotingThread = $derived(
    promoting?.thread_id ? (ai?.threads.find((t) => t.id === promoting?.thread_id) ?? null) : null,
  );

  function submitPromote(body: string) {
    if (!promoting) return;
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked("promote_finding_to_comment");
    void app.cmd("promote_finding_to_comment", { findingId: promoting.id, body });
    promoting = null;
  }
</script>

<section aria-label="Review findings" data-testid="overview-findings">
  <div class="mb-2.5 flex flex-wrap items-center gap-x-2.5 gap-y-1">
    <h2 class="m-0 text-[11px] font-medium uppercase tracking-[.06em] text-fg-3">Review findings</h2>
    {#if hasReview && ai && !ai.fresh}
      <span
        class="inline-flex h-5 items-center rounded-[5px] bg-warning/14 px-[7px] text-[11px] font-medium text-warning"
        title={ai.stale_reason ?? "The review was built from an older diff"}
      >stale</span>
    {:else if hasReview && age}
      <span class="mono text-[11px] text-fg-3">fresh · {age}</span>
    {/if}
    {#if hasReview}
      <span class="mono inline-flex gap-2 text-[11px]">
        {#each SEVERITIES as sev (sev)}
          <span class={SEV_TEXT[sev]}>{view.counts[sev]} {sev}</span>
        {/each}
        {#if view.resolvedCount > 0}<span class="text-fg-3">{view.resolvedCount} resolved</span>{/if}
      </span>
    {/if}
    <span class="flex-1"></span>
    {#if caption}
      <span class="text-[12px] text-fg-3 max-[880px]:hidden">{caption}</span>
    {/if}
  </div>

  {#if !hasReview}
    <div class="rounded-[10px] border border-line bg-surface px-4 py-3.5 text-[12.5px] text-fg-3">
      No review in this view yet.
    </div>
  {:else}
    <div class="overflow-hidden rounded-[10px] border border-line bg-surface {ai?.fresh ? '' : 'opacity-[.72]'}">
      {#each view.rows as finding (finding.id)}
        <div
          class="grid grid-cols-[3px_minmax(0,1fr)] border-t border-line first:border-t-0 {finding.resolved ? 'opacity-50' : ''}"
          data-testid="overview-finding"
        >
          <span class={SEV_BG[finding.severity]}></span>
          <div class="grid min-w-0 gap-[5px] px-3.5 pt-[11px] pb-2.5">
            <div class="flex flex-wrap items-baseline gap-2">
              <span class="inline-flex items-center gap-1.5 text-[11px] font-medium {SEV_TEXT[finding.severity]}">
                <i class="h-[7px] w-[7px] rounded-[2px] {SEV_BG[finding.severity]}"></i>{finding.severity}
              </span>
              <span class="text-[13.5px] font-semibold text-fg {finding.resolved ? 'line-through' : ''}">{finding.title}</span>
              <span
                class="text-[11px] {finding.confidence === 'confirmed' && !finding.resolved ? 'text-add-fg' : 'text-fg-3'}"
                title="Confidence: {finding.confidence}"
              >{confidenceWord(finding)}</span>
              <button
                type="button"
                class="mono ml-auto text-[11.5px] text-fg underline decoration-dotted underline-offset-[3px] hover:text-accent"
                title={fileLine(finding)}
                onclick={() => void open(finding)}
              >{findingLocation(baseName(finding.file), finding.line, 28)}</button>
            </div>
            {#if finding.message_markdown}
              <MarkdownText text={finding.message_markdown} className="max-w-[72ch] text-[13px] leading-[1.55] text-fg-2" />
            {/if}
            <div class="flex flex-wrap items-center gap-1.5 text-[11.5px] text-fg-3">
              {#each findingAgentLabels(finding) as lens (lens)}
                <span class="mono rounded border border-border px-[5px] text-[10.5px] text-fg-2" title="Lens">{lens}</span>
              {/each}
              {#if finding.providers.length > 0}
                <span title="Providers that raised it">{[...new Set(finding.providers)].join(", ")}</span>
              {/if}
              <span class="ml-auto flex gap-0.5">
                {#if canPromote(finding, onPrView)}
                  <button
                    type="button"
                    class="rounded-[5px] px-[7px] py-0.5 text-fg-2 hover:bg-hover hover:text-fg"
                    title="Turns this finding into a GitHub comment and removes the finding"
                    onclick={() => (promoting = finding)}
                  >Promote</button>
                {/if}
                <button
                  type="button"
                  class="rounded-[5px] px-[7px] py-0.5 text-fg-2 hover:bg-hover hover:text-fg"
                  title={finding.resolved ? "Reopen this finding" : "Mark this finding resolved; it stays listed, dimmed"}
                  onclick={() => toggleResolved(finding)}
                >{finding.resolved ? "Reopen" : "Resolve"}</button>
              </span>
            </div>
          </div>
        </div>
      {:else}
        <p class="m-0 px-3.5 py-3 text-[12.5px] text-fg-3">No findings in this review.</p>
      {/each}
    </div>
    {#if view.gatedCount > 0}
      <!-- Said out loud because a list that is quietly shorter is how people stop trusting it. -->
      <p class="m-0 mt-1.5 text-[11.5px] text-fg-3">{view.gatedCount} below the confidence gate</p>
    {/if}
  {/if}
</section>

<PromoteModal
  open={promoting != null}
  kind="finding"
  sourceId={promoting?.id ?? ""}
  initialBody={promoting ? promoteBody(promoting, promotingThread) : ""}
  targetLineLabel={promoting ? fileLine(promoting) : ""}
  onSubmit={submitPromote}
  onClose={() => (promoting = null)}
/>
