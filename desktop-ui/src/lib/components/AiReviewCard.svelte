<script lang="ts">
  import type { AiSnapshot, Confidence, FlatFinding } from "$lib/types";
  import { confidenceGlyph, findingPassesTrust, TRUST_RANK } from "$lib/diffAnnotations";
  import { findingsVisibility } from "$lib/stores/findingsVisibility.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { navigateToFinding } from "$lib/dom";
  import { copyToClipboard } from "$lib/clipboard";
  import {
    ALL_REVIEWERS,
    agentPillStyle,
    filterByAgent,
    findingAgentLabel,
    EMPTY_FINDINGS_STATUS,
    resolveAgentSummary,
    reviewFindingCounts,
    showEmptyFindingsStatus,
    uniqueAgentLabels,
    useAgentScopedSummary,
  } from "$lib/aiReviewAgents";
  import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";
  import { arenaLog } from "$lib/arena/log";
  import ArenaHistoryList from "$lib/components/arena/ArenaHistoryList.svelte";
  import CardDeleteButton from "$lib/components/ui/CardDeleteButton.svelte";
  import { arena } from "$lib/stores/arena.svelte";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  let open = $state(false);
  let summaryOpen = $state(false);
  let staleHelpOpen = $state(false);
  let filter = $state<"all" | "high" | "med" | "low">("all");

  const agentLabels = $derived(
    uniqueAgentLabels(ai.findings, Object.keys(ai.agent_summaries ?? {})),
  );

  const fileRisks = $derived(ai.file_risks ?? []);

  $effect(() => {
    ai.findings;
    ai.agent_summaries;
    aiReviewFilter.syncFromFindings(
      ai.findings,
      Object.keys(ai.agent_summaries ?? {}),
    );
  });

  function basename(p: string): string {
    const i = p.lastIndexOf("/");
    return i === -1 ? p : p.slice(i + 1);
  }

  function jumpTo(finding: (typeof filtered)[0]) {
    navigateToFinding(finding);
  }

  const agentScopedFindings = $derived(
    filterByAgent(ai.findings, aiReviewFilter.filter),
  );
  const scopedCounts = $derived(
    reviewFindingCounts(ai, aiReviewFilter.filter),
  );
  const showAgentDropdown = $derived(
    ai.findings.length > 0 || agentLabels.length > 1,
  );
  const showAgentPills = $derived(
    aiReviewFilter.filter === ALL_REVIEWERS && agentLabels.length > 1,
  );
  const agentSummaryOnly = $derived(useAgentScopedSummary(aiReviewFilter.filter));

  const isEmpty = $derived(
    ai.findings.length === 0 &&
      scopedCounts.high + scopedCounts.med + scopedCounts.low === 0
  );

  const hasReviewContent = $derived(
    !isEmpty ||
      !!ai.summary_markdown ||
      ai.has_review_json ||
      fileRisks.length > 0,
  );

  const resolvedSummary = $derived(
    resolveAgentSummary(
      ai,
      aiReviewFilter.filter,
      scopedCounts,
      new Set(agentScopedFindings.map((f) => f.file)).size,
      isEmpty,
      fileRisks.length,
    ),
  );
  const summary = $derived(resolvedSummary.text);
  const summaryIsMarkdown = $derived(resolvedSummary.markdown);
  const emptyFindingsStatus = $derived(
    showEmptyFindingsStatus(isEmpty, resolvedSummary),
  );
  const staleReason = $derived(ai.stale_reason ?? "Review artifacts are stale.");

  const branchArenaRuns = $derived(arena.branchSummaries);
  const latestArena = $derived(branchArenaRuns[0] ?? null);
  const arenaRunning = $derived(arena.hasLiveRun);
  const activeArenaRunId = $derived(app.snapshot?.active_arena_run ?? arena.liveRunId);

  const filtered = $derived(
    agentScopedFindings.filter((f) => filter === "all" || f.severity === filter)
  );

  /// The gate in force: the reader's override, else the engine's default for
  /// this review — tighter once an arbiter has graded it.
  const minTrust = $derived(findingsVisibility.minTrust(ai.min_trust_default));

  /// What the list draws, and how much the gate is holding back. A filter that
  /// hides rows without saying so is how people stop trusting it.
  const visible = $derived(filtered.filter((f) => findingPassesTrust(f, minTrust)));
  const hiddenCount = $derived(filtered.length - visible.length);

  /// Severity first, then how much the grade can be trusted — the order the TUI
  /// panel draws. Without it the rows come out in whatever order the review's
  /// file map iterated in.
  const SEVERITY_ORDER: Record<FlatFinding["severity"], number> = { high: 0, med: 1, low: 2 };
  const ordered = $derived(
    [...visible].sort(
      (a, b) =>
        SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity] ||
        TRUST_RANK[a.confidence] - TRUST_RANK[b.confidence] ||
        a.file.localeCompare(b.file),
    ),
  );

  /// The arbiter's removals, listed on demand behind the count.
  const droppedFindings = $derived(ai.dropped_findings ?? []);
  let showDropped = $state(false);

  /// A row the arbiter regraded carries its ruling in the response trail, which
  /// is the only place the previous grade survives. The backend tags the entry,
  /// so this reads a field rather than the sentence inside it.
  function arbiterRuling(finding: FlatFinding): string | null {
    return (finding.responses ?? []).find((r) => r.origin === "arbiter")?.body_markdown ?? null;
  }

  const GATE_OPTIONS: { level: Confidence; label: string; hint: string }[] = [
    { level: "informational", label: "all", hint: "Show every grade, including informational" },
    { level: "tentative", label: "tentative+", hint: "Hide informational findings" },
    { level: "confirmed", label: "confirmed", hint: "Show only confirmed findings" },
  ];

  function revealErFolder() {
    invoke("reveal_er_folder").catch(() => {});
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
        })
      );
      const findingCount = Object.values(files).reduce(
        (n, entry) => n + entry.findings.length,
        0
      );
      const payload = {
        diff_hash: review.diff_hash,
        diff_scope: review.diff_scope,
        base_branch: review.base_branch,
        head_branch: review.head_branch,
        files,
      };
      await copyToClipboard(JSON.stringify(payload, null, 2));
      app.showToast("success", `Copied ${findingCount} findings as JSON`);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      app.showToast("error", msg.includes("review.json") ? "No review.json found" : msg);
    }
  }

  async function discardReview() {
    try {
      await app.cmd("delete_review_artifact", { kind: "review" });
      app.showToast("success", "Review cleared");
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      app.showToast("error", msg);
    }
  }
</script>

<Card class="group">
  {#if arena.enabled}
    <div
      class="mb-4 rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)] p-3"
    >
      <div class="flex items-start justify-between gap-2">
        <div>
          <p class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-periwinkle)]">
            AI Review Arena
          </p>
          {#if latestArena}
            <p class="mt-1 text-[12px] font-medium text-[var(--arena-fg)]">
              {latestArena.title ?? latestArena.id}
            </p>
            <p class="text-[10px] text-[var(--arena-fg-subtle)]">
              {latestArena.finding_count} findings · {latestArena.reviewer_count} reviewers
            </p>
          {:else}
            <p class="mt-1 text-[12px] text-[var(--arena-fg-muted)]">
              Multi-reviewer consensus on this diff
            </p>
          {/if}
        </div>
        <div class="flex shrink-0 flex-col items-end gap-1">
          <button
            type="button"
            class="text-[10px] font-semibold text-[var(--arena-periwinkle)] hover:underline"
            onclick={() => arena.openLauncher()}
          >
            + New arena
          </button>
          <button
            type="button"
            class="text-[10px] font-semibold text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)] hover:underline"
            onclick={() => arena.openSingleReviewLauncher()}
          >
            Single review
          </button>
        </div>
      </div>
      <div class="mt-3 flex flex-wrap gap-2">
        {#if latestArena}
          <Button
            variant="secondary"
            onclick={() =>
              arenaRunning ? arena.showRunningProgress() : arena.openOverlay(latestArena.id)}
          >
            {arenaRunning ? "View progress" : "Open latest"}
          </Button>
        {/if}
        <button
          type="button"
          onclick={() => {
            arenaLog("AiReviewCard: Promote to Arena clicked");
            arena.openLauncher();
          }}
          class="rounded-md border border-[var(--arena-border)] px-3 py-1.5 text-[11px] text-[var(--arena-fg-muted)] hover:bg-[var(--arena-bg-2)]"
        >
          {isEmpty ? "Run as Arena" : "Promote to Arena"}
        </button>
      </div>
      <ArenaHistoryList
        summaries={branchArenaRuns}
        activeRunId={activeArenaRunId}
        onOpen={(id) => void arena.openOverlay(id)}
        onDelete={(id) => void arena.deleteRun(id)}
      />
    </div>
  {/if}

  {#if showAgentDropdown}
    <select
      bind:value={aiReviewFilter.filter}
      class="w-full mb-2 bg-bg border border-border rounded px-2 py-1.5 text-xs text-fg outline-none"
      aria-label="Review agent"
    >
      {#if agentLabels.length > 1}
        <option value={ALL_REVIEWERS}>All reviewers</option>
      {/if}
      {#each agentLabels as label (label)}
        <option value={label}>{label}</option>
      {/each}
    </select>
  {/if}

  <div class="flex items-center justify-between mb-2 gap-2">
    <SectionLabel>AI Review</SectionLabel>
    <div class="flex items-center gap-1 shrink-0">
      {#if hasReviewContent}
        <CardDeleteButton label="Clear review" onDelete={discardReview} />
      {/if}
      {#if ai.fresh}
        <span class="text-[10px] mono text-add-fg">fresh</span>
      {:else}
        <div class="flex items-center gap-1">
          <span class="text-[10px] mono text-ai">stale</span>
          <button
            type="button"
            class="text-[10px] mono text-ai hover:text-fg-2"
            title={staleReason}
            aria-label={staleReason}
            aria-expanded={staleHelpOpen}
            onclick={() => staleHelpOpen = !staleHelpOpen}
          >?</button>
        </div>
      {/if}
    </div>
  </div>
  {#if !ai.fresh && staleHelpOpen}
    <div class="mb-2 rounded border border-hairline bg-bg px-2 py-1.5 text-[11px] text-fg-2">
      {staleReason}
    </div>
  {/if}
  {#if summaryOpen || isEmpty || agentSummaryOnly}
    <div class="summary-expanded mb-3">
      {#if emptyFindingsStatus}
        <p class="mb-2 text-sm font-medium text-add-fg">{EMPTY_FINDINGS_STATUS}</p>
      {/if}
      {#if summaryIsMarkdown}
        <MarkdownText text={summary} className="text-sm text-fg-2 leading-relaxed" />
      {:else if summary !== EMPTY_FINDINGS_STATUS}
        <p class="text-sm text-fg-2 leading-relaxed">{summary}</p>
      {/if}
    </div>
  {:else}
    <div class="summary-preview mb-3 text-sm text-fg-2 leading-relaxed">
      {#if summaryIsMarkdown}
        <MarkdownText text={summary} />
      {:else}
        <p>{summary}</p>
      {/if}
    </div>
  {/if}

  {#if !isEmpty && !agentSummaryOnly}
    <Button
      variant="secondary"
      onclick={() => summaryOpen = !summaryOpen}
      class="w-full mb-3"
    >
      {summaryOpen ? "Hide summary" : "Show summary"}
    </Button>
  {/if}

  <div class="grid grid-cols-3 gap-2 mb-3">
    <button
      onclick={() => { open = true; filter = "high"; }}
      class="rounded-md bg-bg border px-2 py-1.5 text-left hover:border-risk-high {filter === 'high' && open ? 'border-risk-high' : 'border-border'}"
    >
      <div class="flex items-center gap-1.5 text-[10px] text-risk-high uppercase tracking-wider"><span class="w-1.5 h-1.5 rounded-full bg-risk-high"></span>High</div>
      <div class="text-lg font-semibold mono">{scopedCounts.high}</div>
    </button>
    <button
      onclick={() => { open = true; filter = "med"; }}
      class="rounded-md bg-bg border px-2 py-1.5 text-left hover:border-risk-med {filter === 'med' && open ? 'border-risk-med' : 'border-border'}"
    >
      <div class="flex items-center gap-1.5 text-[10px] text-risk-med uppercase tracking-wider"><span class="w-1.5 h-1.5 rounded-full bg-risk-med"></span>Med</div>
      <div class="text-lg font-semibold mono">{scopedCounts.med}</div>
    </button>
    <button
      onclick={() => { open = true; filter = "low"; }}
      class="rounded-md bg-bg border px-2 py-1.5 text-left hover:border-risk-low {filter === 'low' && open ? 'border-risk-low' : 'border-border'}"
    >
      <div class="flex items-center gap-1.5 text-[10px] text-risk-low uppercase tracking-wider"><span class="w-1.5 h-1.5 rounded-full bg-risk-low"></span>Low</div>
      <div class="text-lg font-semibold mono">{scopedCounts.low}</div>
    </button>
  </div>

  {#if open}
    <div class="mt-4 pt-3 border-t border-hairline mb-3">
      <div class="flex items-center gap-1.5 mb-2 text-[10px] mono">
        <button onclick={() => filter = "all"} class="px-2 py-0.5 rounded {filter === 'all' ? 'bg-hairline text-fg' : 'text-fg-3 hover:bg-hover'}">all</button>
        <button onclick={() => filter = "high"} class="px-2 py-0.5 rounded flex items-center gap-1 {filter === 'high' ? 'bg-hairline text-risk-high' : 'text-fg-3 hover:bg-hover'}"><span class="w-1.5 h-1.5 rounded-full bg-risk-high"></span>high</button>
        <button onclick={() => filter = "med"} class="px-2 py-0.5 rounded flex items-center gap-1 {filter === 'med' ? 'bg-hairline text-risk-med' : 'text-fg-3 hover:bg-hover'}"><span class="w-1.5 h-1.5 rounded-full bg-risk-med"></span>med</button>
        <button onclick={() => filter = "low"} class="px-2 py-0.5 rounded flex items-center gap-1 {filter === 'low' ? 'bg-hairline text-risk-low' : 'text-fg-3 hover:bg-hover'}"><span class="w-1.5 h-1.5 rounded-full bg-risk-low"></span>low</button>
      </div>

      <!-- Confidence gate. The default follows the review — it tightens only
           once an arbiter has graded the diff — so the row says which is in
           force rather than just being a filter you did not set. -->
      <div class="flex items-center gap-1.5 mb-2 text-[10px] mono flex-wrap">
        <span class="text-fg-3" title="How much a finding's grade can be trusted">confidence</span>
        {#each GATE_OPTIONS as option (option.level)}
          <button
            title={option.hint}
            onclick={() =>
              findingsVisibility.setMinTrust(
                option.level === ai.min_trust_default ? null : option.level,
              )}
            class="px-2 py-0.5 rounded {minTrust === option.level
              ? 'bg-hairline text-fg'
              : 'text-fg-3 hover:bg-hover'}"
          >{option.label}</button>
        {/each}
        {#if hiddenCount > 0}
          <span class="text-fg-3" title="Below the confidence gate">{hiddenCount} hidden</span>
        {/if}
      </div>

      {#if ai.arbiter_unmatched > 0}
        <!-- The arbiter's grades exist but no longer describe this review, so
             nothing was applied. Saying so beats a card that looks unchanged. -->
        <p class="mb-1.5 text-[10px] text-risk-med">
          {ai.arbiter_unmatched} arbiter verdict{ai.arbiter_unmatched === 1 ? "" : "s"} no longer
          apply — re-run validation
        </p>
      {/if}

      {#if ai.arbiter_dropped > 0 || ai.arbiter_merged > 0 || ai.arbiter_regraded > 0}
        <!-- A list that is quietly shorter is how people stop trusting it, so
             the arbiter's rulings are counted rather than silently omitted —
             and the claims it removed stay readable behind the count. -->
        <button
          type="button"
          class="mb-1.5 text-[10px] text-fg-3 text-left {droppedFindings.length > 0
            ? 'hover:text-fg cursor-pointer'
            : 'cursor-default'}"
          disabled={droppedFindings.length === 0}
          onclick={() => (showDropped = !showDropped)}
        >
          {[
            ai.arbiter_dropped > 0 ? `${ai.arbiter_dropped} dropped` : "",
            ai.arbiter_merged > 0 ? `${ai.arbiter_merged} merged` : "",
            ai.arbiter_regraded > 0 ? `${ai.arbiter_regraded} regraded` : "",
          ]
            .filter(Boolean)
            .join(", ")} by arbiter{#if droppedFindings.length > 0}<span class="ml-1 underline decoration-dotted">{showDropped ? "hide" : "show"}</span>{/if}
        </button>
        {#if showDropped}
          <ul class="mb-2 space-y-0.5">
            {#each droppedFindings as dropped (dropped.id)}
              <li>
                <button
                  type="button"
                  onclick={() => jumpTo(dropped)}
                  class="w-full text-left text-[10px] text-fg-3 opacity-70 hover:opacity-100"
                >
                  <span class="mono">{basename(dropped.file)}{dropped.line !== null ? `:${dropped.line}` : ""}</span>
                  — {dropped.title}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}

      <div class="findings-list space-y-1.5">
      {#each ordered as finding (finding.id)}
        {@const dotClass = finding.severity === "high" ? "bg-risk-high" : finding.severity === "med" ? "bg-risk-med" : "bg-risk-low"}
        {@const label = findingAgentLabel(finding)}
        <div class="relative group">
          <button
            onclick={() => jumpTo(finding)}
            class="w-full text-left p-2 pr-6 rounded-md hover:bg-bg border border-transparent hover:border-border block"
          >
            <div class="flex items-start gap-2">
              <span class="w-1.5 h-1.5 rounded-full mt-1.5 shrink-0 {dotClass}"></span>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-1.5 flex-wrap mb-0.5">
                  <span class="text-[11px] font-mono text-muted">{basename(finding.file)}{finding.line !== null ? `:${finding.line}` : ""}</span>
                  <span
                    class="px-1 py-0.5 rounded border border-hairline text-[9px] text-muted shrink-0"
                    title="Confidence: {finding.confidence}"
                  >{confidenceGlyph(finding.confidence)}</span>
                  {#if arbiterRuling(finding)}
                    <span
                      class="px-1 py-0.5 rounded border border-hairline text-[9px] text-ai"
                      title={arbiterRuling(finding)}
                    >regraded</span>
                  {/if}
                  {#if showAgentPills}
                    <span
                      class="px-1 py-0 rounded-full text-[9px] font-medium border shrink-0"
                      style={agentPillStyle(label)}
                    >{label}</span>
                  {/if}
                  {#if finding.raised_by.length > 1}
                    <!-- Several experts independently found this, which is why
                         they are one row — say so rather than showing only the
                         lens it happens to be filed under. -->
                    <span class="text-[9px] text-fg-3 shrink-0"
                      >raised by {finding.raised_by.join(", ")}</span
                    >
                  {/if}
                </div>
                <div class="text-[13px] text-fg-2 leading-snug">{finding.title}</div>
              </div>
            </div>
          </button>
          <button
            type="button"
            onclick={() => app.cmd("dismiss_finding", { findingId: finding.id })}
            title="Dismiss finding"
            class="absolute top-1.5 right-1 p-0.5 rounded opacity-0 group-hover:opacity-100 transition hover:bg-del-bg text-muted hover:text-del-fg"
          >
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>
          </button>
        </div>
      {/each}
      </div>
    </div>
  {/if}

  {#if !isEmpty}
    <Button
      variant="secondary"
      onclick={() => open = !open}
      class="w-full flex items-center justify-center gap-2 normal-case"
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="transition-transform {open ? 'rotate-180' : ''}">
        <polyline points="6 9 12 15 18 9"/>
      </svg>
      <span>{open ? "Hide findings" : "View findings"}</span>
    </Button>
  {/if}

  <div class="mt-2 flex flex-col gap-1">
    <!-- The cheap path: triage picks the lenses, the experts run, and this makes
         one arbiter pass over what they produced — merging duplicates and
         regrading confidence. -->
    <button
      type="button"
      onclick={() => arena.validateFindings()}
      disabled={!(ai.has_review_json || Object.keys(ai.agent_summaries).length > 0)}
      class="w-full flex items-center justify-center gap-2 text-[11px] mono text-fg-3 hover:text-fg py-1.5 rounded hover:bg-bg border border-transparent hover:border-border disabled:opacity-40 disabled:pointer-events-none"
      title={Object.keys(ai.agent_summaries).length > 0
        ? "Merge duplicate findings and regrade confidence with one arbiter pass over the expert output"
        : "Run the expert reviewers first — this validates what they produced"}
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0" aria-hidden="true">
        <path d="M20 6L9 17l-5-5"/>
      </svg>
      <span class="whitespace-nowrap">Validate findings</span>
    </button>
    <button
      type="button"
      onclick={copyFindingsJson}
      disabled={isEmpty}
      class="w-full flex items-center justify-center gap-2 text-[11px] mono text-fg-3 hover:text-fg py-1.5 rounded hover:bg-bg border border-transparent hover:border-border disabled:opacity-40 disabled:pointer-events-none"
      title="Copy findings from review.json for pasting into an agent prompt"
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0" aria-hidden="true">
        <rect x="9" y="9" width="13" height="13" rx="2"/>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
      </svg>
      <span class="whitespace-nowrap">Copy findings JSON</span>
    </button>
    <button
      type="button"
      onclick={revealErFolder}
      class="w-full flex items-center justify-center gap-2 text-[11px] mono text-fg-3 hover:text-fg py-1.5 rounded hover:bg-bg border border-transparent hover:border-border"
      title="Open the review files folder in your file manager"
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0" aria-hidden="true">
        <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2z"/>
      </svg>
      <span class="whitespace-nowrap">Reveal review files</span>
    </button>
  </div>
</Card>

<style>
  .summary-preview {
    display: -webkit-box;
    overflow: hidden;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .summary-expanded {
    max-height: 20rem;
    overflow-y: auto;
  }

  .findings-list {
    max-height: 22rem;
    overflow-y: auto;
  }
</style>
