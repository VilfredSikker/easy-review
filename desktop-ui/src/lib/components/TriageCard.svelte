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
  import {
    evidencedGuard,
    filesPreview,
    followUpFor,
    hasReachDetail,
    previewLine,
    priorityDotClass,
    reachPreview,
    summaryPills,
    verdictLabel,
  } from "$lib/triageCard";
  import { tick } from "svelte";

  interface Props {
    triage: TriageSnapshot;
  }

  const { triage }: Props = $props();

  let open = $state(true);

  // Every row starts collapsed: the pills and the row teasers are the first
  // read, and a row opens only when the reader wants its reasoning.
  let verdictOpen = $state(false);
  let filesOpen = $state(false);
  let reachOpen = $state(false);
  let impressionOpen = $state(false);

  const reviewScope = $derived(reviewScopeFromMode(app.snapshot?.mode));
  const label = $derived(verdictLabel(triage.verdict_primary));
  const pills = $derived(summaryPills(triage));

  /** A guard counts only with the line where it is checked (ADR 0039). */
  const guard = $derived(evidencedGuard(triage));
  /** Triage from before reach existed, or a reach with nothing to say, gets no row. */
  const showReach = $derived(hasReachDetail(triage));
  const touchPoints = $derived(triage.touch_points ?? []);

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

  function runFollowUp() {
    if (!reviewScope) return;
    const next = followUpFor(triage, reviewScope);
    if (!next) return;
    if (next.kind === "arena") {
      arena.openLauncher();
      return;
    }
    void app.cmd(next.command, next.args);
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

<Card class="triage-card group flex max-h-[360px] min-w-0 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center justify-between gap-2">
    <button
      type="button"
      class="flex min-w-0 flex-1 items-center justify-between gap-2 text-left"
      aria-expanded={open}
      onclick={() => (open = !open)}
    >
      <SectionLabel>Triage</SectionLabel>
      <span class="rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-wide
        {triage.fresh ? 'border-info/30 bg-info/10 text-info' : 'border-warning/30 bg-warning/10 text-warning'}">
        {triage.fresh ? label : "stale"}
      </span>
    </button>
    <CardDeleteButton label="Discard triage" onDelete={discardTriage} />
  </div>

  {#if open}
    <div class="mt-2.5 min-h-0 flex-1 overflow-y-auto overflow-x-hidden">
      {#if pills.length > 0}
        <div class="flex flex-wrap gap-1" data-testid="triage-pills">
          {#each pills as pill (pill.label)}
            <Pill tone={pill.tone} title={pill.title}>{pill.label}</Pill>
          {/each}
        </div>
      {/if}

      {#if triage.domains.length > 0}
        <ul class="mt-1.5 flex flex-wrap gap-1" aria-label="Domains">
          {#each triage.domains as domain (domain)}
            <li class="rounded bg-hairline px-1 py-px font-mono text-[9px] text-fg-3">{domain}</li>
          {/each}
        </ul>
      {/if}

      <div class="mt-2.5 min-w-0">
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

        {#if showReach}
          <Disclosure
            label="Reach"
            preview={reachPreview(triage)}
            bind:open={reachOpen}
          >
            {#if triage.reach_reason}
              <p class="text-[12px] leading-relaxed text-fg-2">{triage.reach_reason}</p>
            {/if}
            {#if guard}
              <p class="mt-1 text-[11px] text-muted">
                <span class="text-success">Guard</span>
                · {guard.kind}{guard.name ? ` ${guard.name}` : ""}
                · <span class="font-mono">{guard.evidence}</span>
              </p>
            {/if}
            {#if touchPoints.length > 0}
              <p class="mt-1.5 mb-0.5 text-[10px] uppercase tracking-wide text-muted">Touch points</p>
              <ul class="space-y-1 font-mono text-[10px] leading-snug text-fg-2">
                {#each touchPoints as tp, i (i)}
                  <li class="break-words">{tp}</li>
                {/each}
              </ul>
            {/if}
          </Disclosure>
        {/if}

        {#if triage.first_impression}
          <Disclosure
            label="First impression"
            preview={previewLine(triage.first_impression)}
            bind:open={impressionOpen}
          >
            <MarkdownText
              text={triage.first_impression}
              className="text-[12px] leading-relaxed text-fg-2"
            />
          </Disclosure>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 flex-wrap gap-2 pt-3">
      {#if showFollowUp}
        <Button size="sm" variant="primary" onclick={runFollowUp}>
          Run {label}
        </Button>
      {/if}
      {#if reviewScope}
        <Button size="sm" variant="ghost" onclick={runTriageAgain}>
          Re-triage
        </Button>
      {/if}
    </div>
  {/if}
</Card>
