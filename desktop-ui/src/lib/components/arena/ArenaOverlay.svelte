<script lang="ts">
  import type { ArenaRunSnapshot, ArenaRunSummary } from "$lib/types/arena";
  import {
    endedRunNotice,
    importableFindings,
    importedCount,
    overlayAccess,
    roundsRun,
    runOrdinals,
    runStatusLabel,
  } from "$lib/arena/arenaView";
  import { effortLabel } from "$lib/arena/effort";
  import { isSeededRunFromSnapshot, isSingleReviewRun } from "$lib/arena/runKind";
  import { arena, isArenaRunActive } from "$lib/stores/arena.svelte";
  import { timeAgo } from "$lib/time";
  import ArenaProcessBracket from "$lib/components/arena/ArenaProcessBracket.svelte";
  import ArenaProcessMatrix from "$lib/components/arena/ArenaProcessMatrix.svelte";
  import ArenaProcessFunnel from "$lib/components/arena/ArenaProcessFunnel.svelte";
  import ArenaFinalTruth from "$lib/components/arena/ArenaFinalTruth.svelte";
  import ArenaFindingDetail from "$lib/components/arena/ArenaFindingDetail.svelte";
  import ArenaStatsStrip from "$lib/components/arena/ArenaStatsStrip.svelte";
  import ArenaVoteLegend from "$lib/components/arena/ArenaVoteLegend.svelte";
  import VerificationView from "$lib/components/arena/VerificationView.svelte";
  import { startWindowDrag } from "$lib/windowDrag";

  export type ArenaLayoutMode = "bracket" | "matrix" | "funnel";

  interface Props {
    open: boolean;
    snapshot: ArenaRunSnapshot;
    layoutMode?: ArenaLayoutMode;
    onClose: () => void;
    onLayoutMode?: (mode: ArenaLayoutMode) => void;
    /**
     * Not called: "+ New arena" opens the launcher over this page, which stays
     * open beneath it. Kept so the host's existing binding still type-checks.
     */
    onNewRun?: () => void;
    flowAcceptance?: boolean;
  }

  let {
    open,
    snapshot,
    layoutMode = $bindable("matrix"),
    onClose,
    onLayoutMode,
    flowAcceptance = false,
  }: Props = $props();

  let pickedId = $state<string | null>(null);
  let importing = $state(false);

  const run = $derived(snapshot.run);
  const isReadOnlyVerification = $derived(isSeededRunFromSnapshot(snapshot));
  const isSingleReview = $derived(isSingleReviewRun(null, snapshot));
  const isSingleColumnLayout = $derived(isSingleReview || isReadOnlyVerification);
  const access = $derived(
    overlayAccess({
      flowAcceptance,
      runKind: run.config.run_kind,
      readOnlyVerification: isReadOnlyVerification,
    }),
  );
  const readOnlyFlow = $derived(access.readOnlyFlow);
  // A seeded run and a review flow's Deep pass are both an arbiter ruling on findings already made.
  const isVerificationPage = $derived(isReadOnlyVerification || run.config.run_kind === "review_flow");
  const readOnly = $derived(access.readOnly);

  // The page always shows a detail; a stale pick from another run falls back to the first finding.
  const selectedId = $derived(
    run.findings.some((f) => f.id === pickedId) ? pickedId : run.findings[0]?.id ?? null,
  );

  /** Arena runs on this branch as tabs; single reviews live in the Review tab, not here. */
  const runTabs = $derived.by((): ArenaRunSummary[] => {
    const list = arena.branchSummaries.filter((s) => s.reviewer_count >= 2 || s.id === run.id);
    if (!list.some((s) => s.id === run.id)) {
      list.push({
        id: run.id,
        title: run.title,
        branch_ref: run.branch_ref,
        status: run.status,
        created_at: run.created_at,
        completed_at: run.completed_at,
        reviewer_count: run.reviewers.length,
        finding_count: run.findings.length,
      });
    }
    return list;
  });
  const ordinals = $derived(runOrdinals(runTabs));
  const tabsNewestFirst = $derived(
    [...runTabs].sort((a, b) => (ordinals.get(b.id) ?? 0) - (ordinals.get(a.id) ?? 0)),
  );
  const ordinal = $derived(ordinals.get(run.id));
  const ended = $derived(endedRunNotice(snapshot, ordinal));
  const fallbackRun = $derived(
    tabsNewestFirst.find((s) => s.id !== run.id && s.status === "complete") ?? null,
  );

  const excluded = $derived(arena.excludedFor(run.id));
  const importable = $derived(importableFindings(run, excluded));
  const imported = $derived(importedCount(run));
  const canImport = $derived(
    !readOnly && !isSingleColumnLayout && run.status === "complete" && importable.length > 0 && !importing,
  );
  const importLabel = $derived.by(() => {
    if (importing) return "Importing…";
    if (importable.length > 0) {
      return `Import ${importable.length} finding${importable.length === 1 ? "" : "s"} into review`;
    }
    if (imported > 0) return "✓ Imported into review";
    return "Nothing to import";
  });

  const ran = $derived(Math.max(1, roundsRun(run.findings)));
  const roundChoices = $derived(Array.from({ length: ran }, (_, i) => i + 1));
  const matrixRound = $derived(Math.min(arena.matrixRound ?? ran, ran));

  const emptyDiff = $derived((run.cost_estimate?.tokens_in ?? 0) === 0);
  const failedReviewers = $derived(
    run.reviewers.flatMap((r) => {
      if (typeof r.status === "object" && r.status !== null && "failed" in r.status) {
        return [`${r.name}: ${r.status.failed.reason}`];
      }
      return [];
    }),
  );

  const heading = $derived.by(() => {
    if (isVerificationPage) return "Verification results";
    if (isSingleReview) return "AI review";
    return "AI review arena";
  });
  const subtitle = $derived.by(() => {
    if (isReadOnlyVerification) {
      return `${run.reviewers.length} contributing lens${run.reviewers.length === 1 ? "" : "es"} · verifier ${run.config.arbiter.model_id}`;
    }
    if (isSingleReview) return run.title ?? run.reviewers[0]?.name ?? "";
    return "Several reviewers work the same diff independently, vote across rounds, and an arbiter rules on what survives.";
  });

  $effect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      // The launcher and the running card sit over the page; Escape belongs to them.
      const covered = arena.launcherOpen || (arena.runningOpen && !arena.runningMinimized);
      if (e.key === "Escape" && !covered) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  function setLayout(mode: ArenaLayoutMode) {
    layoutMode = mode;
    onLayoutMode?.(mode);
  }

  function newRun() {
    arena.openLauncher();
  }

  async function importAll() {
    if (!canImport) return;
    importing = true;
    try {
      await arena.importSurvivors(snapshot);
    } catch {
      // acceptFindings already told the user what failed.
    } finally {
      importing = false;
    }
  }

  function tabTitle(s: ArenaRunSummary): string {
    return [s.title, timeAgo(s.created_at), runStatusLabel(s.status)].filter(Boolean).join(" · ");
  }
</script>

{#if open}
  <div class="ar fixed inset-0 z-[100]" role="dialog" aria-modal="true" aria-label={heading}>
    {#if isVerificationPage && !ended}
    <VerificationView {snapshot} {flowAcceptance} {onClose} />
    {:else}
    <!-- Tauri window-drag region (platform chrome, mouse-only by design); the dialog is aria-labelled -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class="ar-head titlebar-drag"
      style="padding-left: max(20px, env(titlebar-area-x, 80px))"
      data-tauri-drag-region
      onmousedown={startWindowDrag}
    >
      <button type="button" class="btn ghost titlebar-no-drag" onclick={onClose} title="Back (Esc)">‹ Back</button>
      <div class="min-w-0">
        <h2>{heading}</h2>
        <p class="dim">{subtitle}</p>
      </div>
      <span class="flex-1"></span>
      <div class="acts titlebar-no-drag">
        {#if runTabs.length > 1 && !isReadOnlyVerification}
          <div class="seg" role="group" aria-label="Arena runs">
            {#each tabsNewestFirst as s (s.id)}
              <button
                type="button"
                aria-pressed={s.id === run.id}
                title={tabTitle(s)}
                onclick={() => {
                  if (s.id !== run.id) void arena.openOverlay(s.id);
                }}
              >
                Run #{ordinals.get(s.id)}{#if isArenaRunActive(s.status)}<span class="live" aria-label="running"></span>{/if}
              </button>
            {/each}
          </div>
        {/if}
        <button type="button" class="btn" onclick={newRun}>+ New arena</button>
        {#if !isSingleColumnLayout && !ended}
          <button
            type="button"
            class="btn primary"
            disabled={!canImport}
            title={readOnlyFlow
              ? "This historical flow result is read only. Start a new Review to accept findings."
              : undefined}
            onclick={() => void importAll()}
          >
            {readOnlyFlow ? "Read only" : importLabel}
          </button>
        {/if}
      </div>
    </header>

    <div class="ar-body">
      {#if ended}
        <div class="empty">
          <b>{ended.title}</b>
          <p class="dim">{ended.body}</p>
          {#if run.status === "failed" && failedReviewers.length > 0}
            <ul class="dim">
              {#each failedReviewers as line, li (li)}<li>{line}</li>{/each}
            </ul>
          {/if}
          {#if fallbackRun}
            <button type="button" class="btn" onclick={() => void arena.openOverlay(fallbackRun.id)}>
              Back to run #{ordinals.get(fallbackRun.id)}
            </button>
          {/if}
        </div>
      {:else if isSingleColumnLayout}
        <div class="single" class:with-detail={selectedId}>
          <ArenaFinalTruth
            {snapshot}
            {selectedId}
            {flowAcceptance}
            onSelect={(id) => (pickedId = id)}
          />
          {#if selectedId}
            <ArenaFindingDetail {snapshot} findingId={selectedId} readOnly onOpenInDiff={(p) => void arena.openFileInDiff(p)} />
          {/if}
        </div>
      {:else}
        <ArenaStatsStrip {snapshot} effort={run.config.effort ? effortLabel(run.config.effort) : null} />
        {#if run.findings.length === 0}
          <div class="empty">
            {#if emptyDiff}
              <b>No diff was captured for this run</b>
              <p class="dim">
                Scope was <span class="mono">{run.config.scope}</span> against
                <span class="mono">{run.base_branch}</span>, with no changes to review. Start a new arena on
                Unstaged, Staged or selected files.
              </p>
            {:else}
              <b>Reviewers reported no issues</b>
              <p class="dim">The diff had content, but no reviewer proposed a finding.</p>
            {/if}
          </div>
        {:else}
          <div class="ar-split">
            <div class="ar-main">
              <div class="ar-tools">
                <div class="seg" role="group" aria-label="Arena view">
                  {#each [["matrix", "Matrix"], ["bracket", "Bracket"], ["funnel", "Funnel"]] as [id, label] (id)}
                    <button
                      type="button"
                      aria-pressed={layoutMode === id}
                      onclick={() => setLayout(id as ArenaLayoutMode)}>{label}</button
                    >
                  {/each}
                </div>
                {#if layoutMode === "matrix" && roundChoices.length > 1}
                  <div class="seg" role="group" aria-label="Round">
                    {#each roundChoices as n (n)}
                      <button
                        type="button"
                        aria-pressed={matrixRound === n}
                        onclick={() => (arena.matrixRound = n)}>Round {n}</button
                      >
                    {/each}
                  </div>
                {/if}
                <span class="flex-1"></span>
                {#if layoutMode !== "funnel"}<ArenaVoteLegend />{/if}
              </div>
              {#if layoutMode === "matrix"}
                <ArenaProcessMatrix {snapshot} round={matrixRound} {selectedId} onSelect={(id) => (pickedId = id)} />
              {:else if layoutMode === "bracket"}
                <ArenaProcessBracket {snapshot} {selectedId} onSelect={(id) => (pickedId = id)} />
              {:else}
                <ArenaProcessFunnel {snapshot} />
              {/if}
            </div>
            <ArenaFindingDetail
              {snapshot}
              findingId={selectedId}
              {readOnly}
              excluded={selectedId ? excluded.has(selectedId) : false}
              onToggleExcluded={(id) => arena.toggleImportExcluded(run.id, id)}
              onKeepAnyway={access.keepAnyway ? (id) => void arena.keepAnyway(run.id, id) : undefined}
              onOpenInDiff={(p) => void arena.openFileInDiff(p)}
            />
          </div>
        {/if}
      {/if}
    </div>
    {/if}
  </div>
{/if}

<style>
  .ar {
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
    color: var(--color-fg);
    font-size: 13px;
  }
  .ar-head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--color-hairline);
    flex-wrap: wrap;
  }
  .ar-head h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .ar-head p {
    margin: 2px 0 0;
    font-size: 12px;
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .dim {
    color: var(--color-fg-3);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-fg);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--color-fg) 25%, transparent);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn.primary {
    background: var(--color-accent);
    color: var(--color-on-accent);
    border-color: transparent;
  }
  .btn.ghost {
    height: 26px;
    border-color: transparent;
    background: none;
    color: var(--color-fg-3);
  }
  .btn.ghost:hover {
    background: var(--color-card);
    color: var(--color-fg);
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: 7px;
    background: var(--color-card);
  }
  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 9px;
    border-radius: 5px;
    color: var(--color-fg-3);
    font-size: 12px;
    font-weight: 500;
  }
  .seg button[aria-pressed="true"] {
    background: var(--color-surface);
    color: var(--color-fg);
    box-shadow: 0 0 0 1px var(--color-border);
  }
  .live {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-success);
  }
  .ar-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
  .ar-split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 340px;
  }
  .ar-main {
    min-width: 0;
    padding: 14px 20px 24px;
    display: grid;
    gap: 12px;
    align-content: start;
  }
  .ar-tools {
    display: flex;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }
  .single {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
  }
  .single.with-detail {
    grid-template-columns: minmax(0, 1fr) 340px;
  }
  .empty {
    margin: 40px auto;
    max-width: 440px;
    padding: 24px 16px;
    display: grid;
    gap: 10px;
    justify-items: start;
  }
  .empty p,
  .empty ul {
    margin: 0;
    font-size: 12.5px;
  }
  @media (max-width: 1100px) {
    .ar-split,
    .single.with-detail {
      grid-template-columns: minmax(0, 1fr);
    }
    .ar-split :global(.ar-detail),
    .single :global(.ar-detail) {
      border-left: 0;
      border-top: 1px solid var(--color-hairline);
    }
  }
</style>
