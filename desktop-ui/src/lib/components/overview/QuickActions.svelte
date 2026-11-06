<script lang="ts">
  import type { AppSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { exportOptions } from "$lib/stores/exportOptions.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { reviewFlowEstimate } from "$lib/reviewFlow";
  import { shortAge } from "$lib/relativeTime";
  import { resolveContextIdentity } from "$lib/contextIdentity";
  import { resolveTabRoot } from "$lib/resolveTabRoot";
  import { resolveActivePrNumber, resolveActivePrUrl } from "$lib/prUrl";
  import { wantsSource } from "$lib/overviewFileOpen";
  import { copyBranchName, copyPrLink, copyWorktreePath, openPrLink, revealWorktree } from "$lib/paletteQuickActions";
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

  // The branch bar's copy buttons, which the top bar left without a home.
  // ⌘-click goes one step further than copying: reveal the worktree, open the PR.
  const branch = $derived(resolveContextIdentity(snap).branch);
  const activeTab = $derived(snap?.tabs.find((t) => t.is_active) ?? snap?.tabs[snap?.active_tab ?? 0]);
  // A remote PR tab has no checkout, so there is no path to offer.
  const worktree = $derived(resolveTabRoot(snap, activeTab) || null);
  const prUrl = $derived(resolveActivePrUrl(snap));
  const prNumber = $derived(resolveActivePrNumber(snap));

  function onWorktree(e: MouseEvent) {
    if (!worktree) return;
    void (wantsSource(e) ? revealWorktree(worktree) : copyWorktreePath(worktree));
  }

  function onPr(e: MouseEvent) {
    if (!prUrl) return;
    void (wantsSource(e) ? openPrLink(prUrl) : copyPrLink(prUrl));
  }

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

<div class="mt-[18px] flex flex-wrap gap-2" role="group" aria-label="Generate, export and copy">
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

  {#if branch || worktree || prUrl}
    <span class="mx-0.5 w-px self-stretch bg-hairline" aria-hidden="true"></span>
  {/if}
  {#if branch}
    <button type="button" class={qa} title="Copy branch name · {branch}" onclick={() => void copyBranchName(branch)}>
      <span class="{ic} bg-hover text-fg-2">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="6" cy="6" r="2" /><circle cx="6" cy="18" r="2" /><circle cx="18" cy="12" r="2" /><path d="M6 8v8M8 18h2a4 4 0 0 0 4-4v-2" /></svg>
      </span>
      Copy branch
    </button>
  {/if}
  {#if worktree}
    <button type="button" class={qa} title="Click to copy worktree path · ⌘-click to reveal in Finder · {worktree}" onclick={onWorktree}>
      <span class="{ic} bg-hover text-fg-2">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M2 10a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2z" /></svg>
      </span>
      Copy worktree
    </button>
  {/if}
  {#if prUrl}
    <button type="button" class={qa} title="Click to copy PR link · ⌘-click to open on GitHub" onclick={onPr}>
      <span class="{ic} bg-hover text-fg-2">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="18" cy="18" r="3" /><circle cx="6" cy="6" r="3" /><path d="M13 6h3a2 2 0 0 1 2 2v7" /><path d="M6 9v12" /></svg>
      </span>
      Copy PR link
      {#if prNumber}<span class="mono text-[11px] font-normal text-fg-3">#{prNumber}</span>{/if}
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
