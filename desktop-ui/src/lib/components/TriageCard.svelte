<script lang="ts">
  import type { TriageSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import CardDeleteButton from "$lib/components/ui/CardDeleteButton.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { tick } from "svelte";

  interface Props {
    triage: TriageSnapshot;
  }

  const { triage }: Props = $props();

  let open = $state(true);

  const reviewScope = $derived(reviewScopeFromMode(app.snapshot?.mode));

  const verdictLabel = $derived(
    ({
      general: "General review",
      expert: "Expert review",
      arena: "Arena debate",
      professor: "Professor",
      skip: "Skip deep review",
    } as Record<string, string>)[triage.verdict_primary] ?? triage.verdict_primary,
  );

  const verdictSummary = $derived.by(() => {
    const parts = [`Next: ${verdictLabel}`];
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
    const scope = reviewScope;
    switch (triage.verdict_primary) {
      case "general":
        void app.cmd("run_ai_review", { scope });
        break;
      case "expert": {
        const kinds =
          triage.experts.length > 0
            ? triage.experts.map((id) => `expert:${id}`)
            : ["expert:security"];
        void app.cmd("run_ai_scoped_review", {
          scope,
          paths: [],
          reviewerKinds: kinds,
        });
        break;
      }
      case "professor":
        void app.cmd("run_ai_professor_review", { scope, focusPrompt: null });
        break;
      case "arena":
        arena.openLauncher();
        break;
      default:
        break;
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

<Card class="triage-card group flex max-h-[360px] min-w-0 flex-col overflow-hidden">
  <div class="flex shrink-0 items-center justify-between gap-2">
    <button
      type="button"
      class="flex min-w-0 flex-1 items-center justify-between gap-2 text-left"
      onclick={() => (open = !open)}
    >
      <SectionLabel>Triage</SectionLabel>
      <span class="rounded border px-1.5 py-0.5 text-[10px] uppercase tracking-wide
        {triage.fresh ? 'border-info/30 bg-info/10 text-info' : 'border-warning/30 bg-warning/10 text-warning'}">
        {triage.fresh ? verdictLabel : "stale"}
      </span>
    </button>
    <CardDeleteButton label="Discard triage" onDelete={discardTriage} />
  </div>

  {#if open}
    <div class="mt-3 min-h-0 flex-1 space-y-3 overflow-y-auto overflow-x-hidden text-[12px] leading-relaxed">
      {#if triage.first_impression}
        <MarkdownText text={triage.first_impression} />
      {/if}

      <div class="flex flex-wrap gap-2 text-[10px] text-muted">
        {#if triage.files_changed > 0}
          <span>{triage.files_changed} files</span>
        {/if}
        {#if triage.approx_risk}
          <span>risk: {triage.approx_risk}</span>
        {/if}
        {#if triage.domains.length > 0}
          <span>{triage.domains.join(", ")}</span>
        {/if}
      </div>

      <div class="space-y-1.5 rounded-md border border-info/20 bg-info/5 px-3 py-2.5">
        <SectionLabel size="sm">Verdict</SectionLabel>
        <p class="font-medium text-fg-1">{verdictSummary}</p>
        {#if triage.verdict_primary === "expert" && triage.experts.length > 0}
          <p class="text-[11px] text-muted">
            Recommended experts: {triage.experts.join(", ")}
          </p>
        {/if}
        {#if triage.rationale}
          <p class="text-fg-2">{triage.rationale}</p>
        {/if}
      </div>

      {#if triage.priority_files.length > 0}
        <div class="min-w-0">
          <p class="mb-1 text-[10px] uppercase tracking-wide text-muted">Priority files</p>
          <ul class="space-y-1">
            {#each triage.priority_files as pf (pf.path)}
              <li class="min-w-0">
                <button
                  type="button"
                  class="block w-full min-w-0 overflow-hidden text-left hover:text-accent transition-colors"
                  title={pf.reason ? `${pf.path} · ${pf.reason}` : pf.path}
                  onclick={() => navigateToPath(pf.path)}
                >
                  <span class="truncate-start block font-mono text-[11px]">
                    <span class="truncate-start-inner">{pf.path}</span>
                  </span>
                  {#if pf.reason}
                    <span class="block truncate text-muted">{pf.reason}</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>

    <div class="flex shrink-0 flex-wrap gap-2 pt-3">
      {#if showFollowUp}
        <Button size="sm" variant="primary" onclick={runFollowUp}>
          Run {verdictLabel}
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
