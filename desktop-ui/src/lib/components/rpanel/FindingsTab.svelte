<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { AiSnapshot, Confidence, FlatFinding } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { findingsVisibility } from "$lib/stores/findingsVisibility.svelte";
  import { confidenceGlyph } from "$lib/diffAnnotations";
  import { navigateToFinding } from "$lib/dom";
  import { copyToClipboard } from "$lib/clipboard";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { baseName } from "$lib/shortFileName";
  import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";
  import {
    ALL_REVIEWERS,
    EMPTY_FINDINGS_STATUS,
    countBySeverity,
    resolveAgentSummary,
    showEmptyFindingsStatus,
  } from "$lib/aiReviewAgents";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import ArenaPanelCard from "$lib/components/arena/ArenaPanelCard.svelte";
  import {
    arbiterSummary,
    findingLens,
    findingLocation,
    findingsPanelView,
    scopeToReviewer,
    SEVERITIES,
    showReviewerPicker,
    toggleSeverity,
  } from "./findingsPanel";
  import { rightPanelFilters } from "./panelFilters.svelte";
  import PanelActions from "./PanelActions.svelte";
  import ReviewFlowStrip from "./ReviewFlowStrip.svelte";

  interface Props {
    ai: AiSnapshot | null;
    currentFile: string | null;
  }

  const { ai, currentFile }: Props = $props();

  const GATE_OPTIONS: { level: Confidence; label: string; hint: string }[] = [
    { level: "informational", label: "all", hint: "Show every grade, including informational" },
    { level: "tentative", label: "tentative+", hint: "Hide informational findings" },
    { level: "confirmed", label: "confirmed", hint: "Show only confirmed findings" },
  ];

  const SEV_TEXT = { high: "text-risk-high", med: "text-risk-med", low: "text-risk-low" } as const;
  const SEV_DOT = { high: "bg-risk-high", med: "bg-risk-med", low: "bg-risk-low" } as const;

  // The gate in force: the reader's override, else the engine's default for
  // this review — tighter once an arbiter has graded it.
  const minTrust = $derived(ai ? findingsVisibility.minTrust(ai.min_trust_default) : "informational");
  // The reviewer filter is the diff's own (aiReviewFilter), so the list and the
  // diff hide the same rows; a single-review import sets it.
  const reviewer = $derived(aiReviewFilter.filter);
  const scope = $derived(scopeToReviewer(ai, reviewer));
  const showPicker = $derived(showReviewerPicker(scope.labels, reviewer));
  const view = $derived(
    findingsPanelView(scope.open, scope.resolved, {
      severity: rightPanelFilters.severity,
      minTrust,
      showResolved: findingsVisibility.showResolved,
    }),
  );
  const arbiterLine = $derived(ai ? arbiterSummary(ai) : null);
  const droppedFindings = $derived(ai?.dropped_findings ?? []);
  let showDropped = $state(false);

  const hasReview = $derived(
    !!ai &&
      (ai.has_review_json ||
        !!ai.summary_markdown?.trim() ||
        ai.findings.length > 0 ||
        ai.resolved_findings.length > 0),
  );

  // The review's own words: the summary markdown, or the selected reviewer's.
  // A review that found nothing is only this, so it opens by itself then.
  const summary = $derived(
    ai
      ? resolveAgentSummary(ai, reviewer, countBySeverity(scope.open), {
          fileCount: view.fileCount,
          isEmpty: view.openCount === 0,
          fileRiskCount: ai.file_risks?.length ?? 0,
        })
      : null,
  );
  const leadWithNoFindings = $derived(
    !!summary && showEmptyFindingsStatus(view.openCount === 0, summary),
  );
  let summaryOpen = $state(false);
  const summaryExpanded = $derived(summaryOpen || view.openCount === 0);

  async function clearReview() {
    try {
      await app.cmd("delete_review_artifact", { kind: "review" });
      app.showToast("success", "Review cleared");
    } catch (e) {
      app.showToast("error", e instanceof Error ? e.message : String(e));
    }
  }
  const flowActive = $derived(app.snapshot?.review_flow != null);

  const reviewScope = $derived(reviewScopeFromMode(app.snapshot?.mode));
  const activeTab = $derived(app.snapshot?.tabs.find((tab) => tab.is_active));
  const hasExpertFindings = $derived((ai?.findings ?? []).some((f) => f.expert_label != null));
  const canValidateGeneralReview = $derived(
    !!ai?.has_review_json && !!reviewScope && !!activeTab && activeTab.kind !== "remote_pr",
  );
  const canValidate = $derived(hasExpertFindings || canValidateGeneralReview);
  const validateHint = $derived.by(() => {
    if (hasExpertFindings) return "Merge duplicate expert findings and regrade confidence with one arbiter pass";
    if (activeTab?.kind === "remote_pr") return "Check out this PR locally to validate its review findings";
    return "Validate the review findings against the current diff and re-anchor their locations";
  });

  function validate() {
    if (!canValidate) return;
    if (hasExpertFindings) {
      void arena.validateFindings();
    } else if (reviewScope) {
      void app.cmd("run_ai_validate", { scope: reviewScope });
    }
    app.showToast("info", `Validation pass started: re-reading code for ${view.openCount} open findings`);
  }

  async function copyFindingsJson() {
    try {
      const raw = await invoke<string>("read_review_json", { revisionId: null });
      const review = JSON.parse(raw) as {
        diff_hash?: string;
        diff_scope?: string;
        base_branch?: string;
        head_branch?: string;
        files?: Record<string, { findings?: unknown[] }>;
      };
      const files = Object.fromEntries(
        Object.entries(review.files ?? {}).flatMap(([path, fileReview]) => {
          const findings = fileReview.findings ?? [];
          return findings.length > 0 ? [[path, { findings }] as const] : [];
        }),
      );
      const count = Object.values(files).reduce((n, entry) => n + entry.findings.length, 0);
      const { diff_hash, diff_scope, base_branch, head_branch } = review;
      await copyToClipboard(JSON.stringify({ diff_hash, diff_scope, base_branch, head_branch, files }, null, 2));
      app.showToast("success", `Copied ${count} findings as JSON`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      app.showToast("error", msg.includes("review.json") ? "No review.json found" : msg);
    }
  }

  function setResolved(finding: FlatFinding, resolved: boolean) {
    void app.cmd(resolved ? "resolve_finding" : "unresolve_finding", { findingId: finding.id });
  }

  function setGate(level: Confidence) {
    if (!ai) return;
    // Picking the review's own default clears the override, so the gate goes
    // back to following the review when an arbiter later regrades it.
    findingsVisibility.setMinTrust(level === ai.min_trust_default ? null : level);
  }
</script>

{#if !hasReview}
  <div class="grid gap-2.5 justify-items-start py-4 px-1">
    <b class="text-[12.5px] font-semibold text-fg">Nothing in this view yet</b>
    <p class="m-0 text-[12.5px] text-fg-3">
      Findings belong to the view that produced them. Run a review here to fill this tab.
    </p>
    <div class="w-full"><ReviewFlowStrip /></div>
  </div>
{:else}
  <div class="flex items-center justify-between gap-2 text-[12.5px] font-medium text-fg">
    <span>{view.openCount} findings across {view.fileCount} files</span>
    {#if ai?.fresh}
      <span class="mono text-[11px] font-normal text-add-fg">fresh</span>
    {:else}
      <span
        class="mono text-[10.5px] font-normal px-1.5 rounded border border-ai/40 text-ai"
        title={ai?.stale_reason ?? "Review artifacts are stale."}
      >stale</span>
    {/if}
  </div>

  {#if flowActive}
    <ReviewFlowStrip />
  {/if}

  {#if showPicker}
    <div class="flex items-center gap-1.5 text-[11.5px]">
      <span class="text-fg-3">reviewer</span>
      <select
        bind:value={aiReviewFilter.filter}
        aria-label="Reviewer"
        class="min-w-0 flex-1 h-6 px-1.5 rounded-[5px] border border-hairline bg-card text-[11.5px] text-fg outline-none"
      >
        <option value={ALL_REVIEWERS}>All reviewers</option>
        {#each scope.labels as label (label)}
          <option value={label}>{label}</option>
        {/each}
        {#if reviewer !== ALL_REVIEWERS && !scope.labels.includes(reviewer)}
          <option value={reviewer}>{reviewer}</option>
        {/if}
      </select>
      {#if reviewer !== ALL_REVIEWERS}
        <button
          type="button"
          title="Show every reviewer's findings, here and in the diff"
          class="px-[7px] py-px mono text-[11px] text-fg-3 hover:text-fg"
          onclick={() => (aiReviewFilter.filter = ALL_REVIEWERS)}
        >clear</button>
      {/if}
    </div>
  {/if}

  {#if summary}
    <div class="grid gap-1 text-[12px] text-fg-2">
      {#if leadWithNoFindings}
        <b class="text-[12px] font-medium text-add-fg">{EMPTY_FINDINGS_STATUS}</b>
      {/if}
      {#if summaryExpanded}
        <div class="max-h-80 overflow-y-auto">
          {#if summary.markdown}
            <MarkdownText text={summary.text} className="leading-relaxed" />
          {:else if summary.text !== EMPTY_FINDINGS_STATUS}
            <p class="m-0 leading-relaxed">{summary.text}</p>
          {/if}
        </div>
      {:else}
        <div class="line-clamp-2 leading-relaxed">
          {#if summary.markdown}<MarkdownText text={summary.text} />{:else}<p class="m-0">{summary.text}</p>{/if}
        </div>
      {/if}
      {#if view.openCount > 0}
        <button
          type="button"
          class="justify-self-start mono text-[11px] text-fg-3 hover:text-fg"
          aria-expanded={summaryOpen}
          onclick={() => (summaryOpen = !summaryOpen)}
        >{summaryOpen ? "hide summary" : "show summary"}</button>
      {/if}
    </div>
  {/if}

  <div class="grid grid-cols-3 gap-1.5">
    {#each SEVERITIES as sev (sev)}
      {@const on = rightPanelFilters.severity === sev}
      <button
        type="button"
        aria-pressed={on}
        onclick={() => (rightPanelFilters.severity = toggleSeverity(rightPanelFilters.severity, sev))}
        class="grid gap-0.5 px-[9px] py-[7px] rounded-lg border bg-card text-left transition-colors {SEV_TEXT[sev]}
          {on ? 'border-current' : 'border-hairline hover:border-border'}"
      >
        <span class="inline-flex items-center gap-1.5 text-[11px] font-medium"><i class="w-[7px] h-[7px] rounded-[2px] {SEV_DOT[sev]}"></i>{sev}</span>
        <b class="mono text-[18px] font-medium text-fg">{view.counts[sev]}</b>
      </button>
    {/each}
  </div>

  <!-- The Gate (CONTEXT.md). Its default follows the review, so the row shows
       which level is in force rather than reading as a filter nobody set. -->
  <div class="flex items-center gap-1 flex-wrap text-[11.5px]">
    <span class="text-fg-3 mr-0.5" title="How much a finding's grade can be trusted">confidence</span>
    {#each GATE_OPTIONS as option (option.level)}
      <button
        type="button"
        title={option.hint}
        aria-pressed={minTrust === option.level}
        onclick={() => setGate(option.level)}
        class="px-[7px] py-px rounded-[5px] mono text-[11px]
          {minTrust === option.level ? 'bg-hover text-fg ring-1 ring-inset ring-border' : 'text-fg-2 hover:text-fg'}"
      >{option.label}</button>
    {/each}
    {#if view.resolvedCount > 0}
      <button
        type="button"
        title="Show findings you resolved, dimmed below the open ones"
        aria-pressed={findingsVisibility.showResolved}
        onclick={() => findingsVisibility.toggleResolved()}
        class="px-[7px] py-px rounded-[5px] mono text-[11px]
          {findingsVisibility.showResolved ? 'bg-hover text-fg ring-1 ring-inset ring-border' : 'text-fg-2 hover:text-fg'}"
      >resolved {view.resolvedCount}</button>
    {/if}
    {#if rightPanelFilters.severity !== "all"}
      <button
        type="button"
        class="ml-auto px-[7px] py-px mono text-[11px] text-fg-3 hover:text-fg"
        onclick={() => (rightPanelFilters.severity = "all")}
      >clear {rightPanelFilters.severity}</button>
    {/if}
  </div>

  {#if view.gatedCount > 0 || arbiterLine || (ai?.arbiter_unmatched ?? 0) > 0}
    <div class="grid gap-0.5 text-[11px] text-fg-3 -mt-1.5">
      {#if view.gatedCount > 0}
        <span>{view.gatedCount} below the confidence gate</span>
      {/if}
      {#if ai && ai.arbiter_unmatched > 0}
        <!-- Grades that no longer describe this review were not applied;
             saying so beats a list that looks unchanged. -->
        <span class="text-risk-med">
          {ai.arbiter_unmatched} arbiter verdict{ai.arbiter_unmatched === 1 ? "" : "s"} no longer apply — re-run validation
        </span>
      {/if}
      {#if arbiterLine}
        <button
          type="button"
          class="text-left {droppedFindings.length > 0 ? 'hover:text-fg' : 'cursor-default'}"
          disabled={droppedFindings.length === 0}
          onclick={() => (showDropped = !showDropped)}
        >
          {arbiterLine}{#if droppedFindings.length > 0}<span class="ml-1 underline decoration-dotted">{showDropped ? "hide" : "show"}</span>{/if}
        </button>
        {#if showDropped}
          {#each droppedFindings as dropped (dropped.id)}
            <button type="button" class="text-left opacity-70 hover:opacity-100" onclick={() => navigateToFinding(dropped)}>
              <span class="mono">{findingLocation(baseName(dropped.file), dropped.line)}</span> — {dropped.title}
            </button>
          {/each}
        {/if}
      {/if}
    </div>
  {/if}

  <div class="grid gap-0.5 -mx-1.5">
    {#each view.rows as finding (finding.id)}
      {@const here = currentFile === finding.file}
      <div class="group relative">
        <button
          type="button"
          onclick={() => navigateToFinding(finding)}
          class="relative w-full grid gap-[3px] px-2 py-[7px] pr-16 rounded-[7px] text-left hover:bg-hover
            {here ? 'bg-accent/8' : ''} {finding.resolved ? 'opacity-50' : ''}"
        >
          {#if here}<span class="absolute left-0 top-2 bottom-2 w-0.5 rounded bg-accent"></span>{/if}
          <span class="flex items-center gap-1.5 min-w-0 text-[11px] text-fg-3">
            <i class="w-1.5 h-1.5 rounded-[2px] shrink-0 {SEV_DOT[finding.severity]}"></i>
            <span class="mono truncate" title={finding.file}>{findingLocation(baseName(finding.file), finding.line)}</span>
            <span
              class="mono text-[10px] w-3.5 h-3.5 inline-grid place-items-center rounded-[3px] border border-border shrink-0"
              title="Confidence: {finding.confidence}"
            >{confidenceGlyph(finding.confidence)}</span>
            <span class="ml-auto shrink-0">{findingLens(finding)}</span>
          </span>
          <span class="text-[12.5px] font-medium leading-snug text-fg {finding.resolved ? 'line-through' : ''}">{finding.title}</span>
        </button>
        <button
          type="button"
          onclick={() => setResolved(finding, !finding.resolved)}
          title={finding.resolved ? "Reopen this finding" : "Mark this finding fixed"}
          class="absolute top-1.5 right-1 px-1.5 py-px rounded-[5px] mono text-[10.5px] text-fg-3 bg-surface border border-hairline
            opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:text-fg transition-opacity"
        >{finding.resolved ? "Reopen" : "Resolve"}</button>
      </div>
    {:else}
      <p class="px-1 py-3 text-[12px] text-fg-3">No findings pass this filter.</p>
    {/each}
  </div>

  <PanelActions
    actions={[
      { glyph: "✓", label: "Validate findings", title: validateHint, disabled: !canValidate, run: validate },
      { glyph: "⧉", label: "Copy findings JSON", title: "Copy findings from review.json for an agent prompt", disabled: view.openCount === 0, run: copyFindingsJson },
      { glyph: "▢", label: "Reveal review files", title: "Open the review files folder", run: () => void invoke("reveal_er_folder").catch(() => {}) },
      { glyph: "×", label: "Clear review", title: "Delete this view's review: its findings and summary", run: () => void clearReview() },
    ]}
  />
{/if}

{#if arena.enabled}
  <ArenaPanelCard />
{/if}
