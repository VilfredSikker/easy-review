<script lang="ts">
  import type { TriageSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { tick } from "svelte";

  interface Props {
    triage: TriageSnapshot;
  }

  const { triage }: Props = $props();

  let open = $state(true);

  const reviewScope = $derived.by(() => {
    const mode = app.snapshot?.mode;
    return mode === "branch" || mode === "unstaged" || mode === "staged" ? mode : null;
  });

  const verdictLabel = $derived(
    ({
      general: "General review",
      expert: "Expert review",
      arena: "Arena debate",
      professor: "Professor",
      skip: "Skip deep review",
    } as Record<string, string>)[triage.verdict_primary] ?? triage.verdict_primary,
  );

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
</script>

<Card class="triage-card">
  <button
    type="button"
    class="w-full flex items-center justify-between gap-2 text-left"
    onclick={() => (open = !open)}
  >
    <SectionLabel>Triage</SectionLabel>
    <span class="text-[10px] uppercase tracking-wide px-1.5 py-0.5 rounded border
      {triage.fresh ? 'text-cyan-400 border-cyan-400/30 bg-cyan-400/10' : 'text-amber-400 border-amber-400/30 bg-amber-400/10'}">
      {triage.fresh ? verdictLabel : "stale"}
    </span>
  </button>

  {#if open}
    <div class="mt-3 space-y-3 text-[12px] leading-relaxed">
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

      {#if triage.rationale}
        <p class="text-fg-2">{triage.rationale}</p>
      {/if}

      {#if triage.priority_files.length > 0}
        <div>
          <p class="text-[10px] uppercase tracking-wide text-muted mb-1">Priority files</p>
          <ul class="space-y-1">
            {#each triage.priority_files as pf (pf.path)}
              <li>
                <button
                  type="button"
                  class="text-left w-full hover:text-accent transition-colors"
                  onclick={() => navigateToPath(pf.path)}
                >
                  <span class="font-mono text-[11px]">{pf.path}</span>
                  {#if pf.reason}
                    <span class="text-muted"> — {pf.reason}</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <div class="flex flex-wrap gap-2 pt-1">
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
    </div>
  {/if}
</Card>
