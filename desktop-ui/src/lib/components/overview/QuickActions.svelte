<script lang="ts">
  import type { AppSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { exportOptions } from "$lib/stores/exportOptions.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { reviewFlowEstimate } from "$lib/reviewFlow";
  import { shortAge } from "$lib/relativeTime";
  import { generatorStates, latestArenaRun, reviewFlowCommand, type Generator, type GeneratorState } from "./overviewModel";

  interface Props {
    /** Defaults to the live snapshot; stories pass one. */
    snapshot?: AppSnapshot | null;
  }

  const { snapshot: snapshotOverride }: Props = $props();
  const snap = $derived(snapshotOverride !== undefined ? snapshotOverride : app.snapshot);

  const scope = $derived(reviewScopeFromMode(snap?.mode));
  const generators = $derived(generatorStates(snap));
  const arenaRun = $derived(latestArenaRun(snap));
  const exportCount = $derived(exportOptions.itemCount);

  const LABEL: Record<Generator, string> = { triage: "Triage", review: "AI review", tour: "Tour" };
  const BUSY: Record<Generator, string> = {
    triage: "Triaging…",
    review: "Reviewing…",
    tour: "Generating tour…",
  };
  const ICON: Record<Generator, string> = {
    triage: "M2 4h12M4 8h8M6 12h4",
    review: "M8 1.5l1.6 4.2 4.4.3-3.4 2.8 1.1 4.3L8 10.8 4.3 13.1l1.1-4.3L2 6l4.4-.3z",
    tour: "M5.6 4H10a2 2 0 010 4H6a2 2 0 000 4h4.4",
  };

  function buttonTitle(g: GeneratorState): string {
    if (!scope) return "Not available in this view";
    if (g.busy) return "Running";
    if (g.confirm) return `Confirm Deep review: ${reviewFlowEstimate(snap?.review_flow) ?? "estimate unavailable"}`;
    return `${g.at ? "Regenerate" : "Run"} ${LABEL[g.id].toLowerCase()}`;
  }

  function caption(g: GeneratorState): string {
    if (g.busy) return BUSY[g.id];
    return g.confirm ? "Confirm Deep review" : LABEL[g.id];
  }

  function run(id: Generator) {
    if (!scope) return;
    if (id === "triage") void app.cmd("run_ai_triage_review", { scope });
    else if (id === "review") {
      const flow = reviewFlowCommand(snap, scope);
      if (flow) void app.cmd(flow.command, flow.args);
    }
    else void app.cmd("generate_tour");
  }

  const qa =
    "qa inline-flex h-[30px] items-center gap-2 whitespace-nowrap rounded-lg border border-border bg-card pl-1.5 pr-2.5 text-[12px] font-medium text-fg hover:border-accent/45 disabled:opacity-50 aria-busy:cursor-progress aria-busy:text-fg-2";
  const ic = "grid h-5 w-5 place-items-center rounded-[5px]";
</script>

<div class="mt-[18px] flex flex-wrap gap-2" role="group" aria-label="Generate and export">
  {#each generators as g (g.id)}
    <button
      type="button"
      class={qa}
      aria-busy={g.busy}
      disabled={!scope}
      title={buttonTitle(g)}
      onclick={() => {
        if (!g.busy) run(g.id);
      }}
    >
      <span class="{ic} bg-accent/14 text-accent">
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" class:qa-spin={g.busy} aria-hidden="true">
          {#if g.busy}
            <path d="M8 2a6 6 0 105.2 3" />
          {:else}
            <path d={ICON[g.id]} />
            {#if g.id === "tour"}<circle cx="4" cy="4" r="1.6" /><circle cx="12" cy="12" r="1.6" />{/if}
          {/if}
        </svg>
      </span>
      {caption(g)}
      {#if g.confirm}
        <span class="mono text-[11px] font-normal text-fg-3">{reviewFlowEstimate(snap?.review_flow) ?? ""}</span>
      {:else if !g.busy}
        <span class="mono text-[11px] font-normal text-fg-3">{g.at ? shortAge(g.at) : "not run"}</span>
      {/if}
    </button>
  {/each}

  <span class="mx-0.5 w-px self-stretch bg-hairline" aria-hidden="true"></span>

  <button
    type="button"
    class={qa}
    title="Copy the review export with your saved export options"
    onclick={() => void exportOptions.quickCopy()}
  >
    <span class="{ic} bg-hover text-fg-2">
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="M5.5 5.5V3.25c0-.4.35-.75.75-.75h6.5c.4 0 .75.35.75.75v6.5c0 .4-.35.75-.75.75H10.5" /><rect x="2.5" y="5.5" width="8" height="8" rx=".75" /></svg>
    </span>
    Copy export
    {#if exportCount != null}<span class="mono text-[11px] font-normal text-fg-3">{exportCount} items</span>{/if}
  </button>
  <button
    type="button"
    class="{qa} pl-2.5"
    title="Choose what goes into the export"
    onclick={() => app.setMainView("export-review")}
  >Export…</button>
  {#if snap?.arena_enabled !== false}
    <button type="button" class={qa} title="Open the AI review arena" onclick={() => arena.openLauncher()}>
      <span class="{ic} bg-question/14 text-question" aria-hidden="true">⚔</span>
      Arena
      {#if arenaRun}<span class="mono text-[11px] font-normal text-fg-3">#{arenaRun.count} · {shortAge(arenaRun.at)}</span>{/if}
    </button>
  {/if}
</div>

<style>
  @media (prefers-reduced-motion: no-preference) {
    .qa-spin {
      animation: qa-spin 0.9s linear infinite;
    }
  }
  @keyframes qa-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
