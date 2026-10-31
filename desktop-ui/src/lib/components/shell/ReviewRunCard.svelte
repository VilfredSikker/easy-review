<script lang="ts">
  import type { BackgroundTaskSnapshot, ReviewFlowSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import { reviewScopeFromMode } from "$lib/reviewScope";
  import { reviewRunCard, type StepState } from "$lib/reviewRunCard";
  import RunningAgentPanel from "$lib/components/RunningAgentPanel.svelte";

  /** One review flow as a single card: stepper, the Deep confirmation or the
   *  result, and the per-task process grouped by stage. */
  interface Props {
    flow: ReviewFlowSnapshot;
    /** Session background tasks, for each row's live log. */
    tasks?: readonly BackgroundTaskSnapshot[];
    erDir?: string | null;
    mode?: string;
    onNavigate?: () => void;
  }

  const { flow, tasks = [], erDir = null, mode, onNavigate }: Props = $props();

  const card = $derived(reviewRunCard(flow));
  let showProcess = $state(true);
  let openLogs = $state<Record<string, boolean>>({});

  function control(
    command: "confirm_review_flow" | "run_standard_review_flow" | "decline_review_flow" | "cancel_review_flow",
  ) {
    if (!erDir) return;
    void app.cmd(command, { erDir });
  }

  function restart() {
    const scope = reviewScopeFromMode(mode);
    if (!scope || !erDir || !card.canRestart) return;
    void app.cmd("start_review_flow", { scope, erDir });
  }

  function openResults() {
    const runId = card.openRunId;
    if (!erDir || !runId) return;
    onNavigate?.();
    void arena.openFlowOverlay(erDir, runId);
  }

  function toggleLog(id: string) {
    openLogs = { ...openLogs, [id]: !openLogs[id] };
  }

  const stepText: Record<StepState, string> = {
    waiting: "text-muted",
    active: "text-fg font-medium",
    done: "text-add-fg",
    failed: "text-del-fg",
    skipped: "text-muted",
  };
  const stepFill: Record<StepState, string> = {
    waiting: "bg-question",
    active: "bg-question",
    done: "bg-add-fg",
    failed: "bg-del-fg",
    skipped: "bg-ink-500",
  };
  const pillClass: Record<string, string> = {
    running: "text-question bg-question/15",
    queued: "text-muted bg-hover",
    waiting: "text-muted bg-hover",
    done: "text-add-fg bg-add-fg/15",
    failed: "text-del-fg bg-del-fg/15",
    cancelled: "text-muted bg-hover",
    skipped: "text-muted bg-hover",
  };
  const depthClass: Record<string, string> = {
    deep: "text-del-fg bg-del-fg/15",
    standard: "text-action bg-action/15",
    quick: "text-add-fg bg-add-fg/15",
  };
</script>

<section class="grid gap-2.5 px-3.5 py-3" aria-label="Review run" data-testid="review-run-card">
  <div class="flex items-start gap-2.5">
    <div class="grid flex-1 min-w-0 gap-px">
      <div class="text-[10.5px] font-semibold uppercase tracking-[.06em] text-question">
        Review{#if card.depth}
          · <span class="rounded px-1 tracking-[.02em] {depthClass[card.depth]}" data-testid="review-depth">{card.depth}</span>
        {/if}
      </div>
      <b class="text-[13px] font-semibold text-fg">{card.head}</b>
      {#if card.sub}<div class="text-[12px] text-muted">{card.sub}</div>{/if}
    </div>
    {#if card.canCancel}
      <button
        type="button"
        class="h-[26px] shrink-0 rounded-md px-2 text-[12px] text-del-fg hover:bg-hover"
        onclick={() => control("cancel_review_flow")}
      >
        Cancel
      </button>
    {/if}
  </div>

  <div class="grid grid-cols-3 gap-1.5" data-testid="review-stepper">
    {#each card.steps as step (step.stage)}
      <div class="grid gap-1 text-[11.5px] {stepText[step.state]}" data-state={step.state}>
        <div class="h-1 overflow-hidden rounded-sm bg-ink-600">
          {#if step.state === "active" && step.fill === 0}
            <i class="block h-full w-1/3 rounded-sm {stepFill[step.state]} motion-safe:animate-[run-indeterminate_1.4s_ease-in-out_infinite]"></i>
          {:else}
            <i class="block h-full rounded-sm transition-[width] duration-200 {stepFill[step.state]}" style:width="{step.fill * 100}%"></i>
          {/if}
        </div>
        <span>{step.state === "done" ? "✓ " : ""}{step.label}</span>
      </div>
    {/each}
  </div>

  {#if card.confirm}
    <div class="grid gap-[7px] rounded-[9px] border border-warning/40 bg-warning/5 px-3 py-2.5 text-[12.5px]" data-testid="review-confirm">
      <div><b>Deep review</b> with {card.confirm.lenses.length} {card.confirm.lenses.length === 1 ? "lens" : "lenses"} and an arbiter</div>
      <div class="flex flex-wrap gap-1.5">
        {#each card.confirm.lenses as lens (lens.id)}
          <span class="inline-flex items-center gap-[5px] rounded-full border border-hairline px-[7px] py-px text-[11.5px]">
            <i class="inline-block h-[7px] w-[7px] rounded-full" style:background={lens.color}></i>{lens.label}
          </span>
        {/each}
      </div>
      {#if card.confirm.estimate}<div class="text-muted">{card.confirm.estimate}</div>{/if}
      <div class="flex flex-wrap gap-1.5">
        <button
          type="button"
          class="h-[26px] rounded-md bg-accent px-2.5 text-[12px] font-medium text-on-accent disabled:opacity-50"
          disabled={!card.canConfirm}
          onclick={() => control("confirm_review_flow")}
        >
          Run deep review
        </button>
        {#if card.canRunStandard}
          <button
            type="button"
            class="h-[26px] rounded-md border border-hairline px-2.5 text-[12px] hover:bg-hover disabled:opacity-50"
            title="Run the same lenses as ordinary finders, without the multi-round arena"
            onclick={() => control("run_standard_review_flow")}
          >
            Run standard instead
          </button>
        {/if}
        <button
          type="button"
          class="h-[26px] rounded-md px-2.5 text-[12px] text-fg-3 hover:bg-hover hover:text-fg disabled:opacity-50"
          disabled={!card.canConfirm}
          onclick={() => control("decline_review_flow")}
        >
          Skip
        </button>
      </div>
    </div>
  {:else if card.done}
    <div class="grid gap-[7px] rounded-[9px] border border-add-fg/35 bg-add-fg/5 px-3 py-2.5 text-[12.5px]" data-testid="review-done">
      <div class="flex flex-wrap items-baseline gap-3.5">
        <span><b class="font-mono text-[16px] text-add-fg">{card.done.kept}</b> kept</span>
        <span><b class="font-mono text-[16px] text-muted">{card.done.dropped}</b> dropped</span>
        <span class="text-muted">{card.done.finders} finders{card.depth ? ` · ${card.depth}` : ""}</span>
      </div>
      {#if card.done.awaitingAccept > 0 && card.openRunId}
        <div class="text-muted">Deep results wait for you before they reach the review.</div>
        <div class="flex flex-wrap gap-1.5">
          <button
            type="button"
            class="h-[26px] rounded-md bg-accent px-2.5 text-[12px] font-medium text-on-accent"
            title="Opens the verification results, where you pick what goes into the review"
            onclick={openResults}
          >
            Accept {card.done.awaitingAccept} {card.done.awaitingAccept === 1 ? "finding" : "findings"}…
          </button>
        </div>
      {:else if card.openRunId}
        <div class="flex flex-wrap gap-1.5">
          <button type="button" class="h-[26px] rounded-md border border-hairline px-2.5 text-[12px] hover:bg-hover" onclick={openResults}>
            Open verification
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if card.groups.length > 0}
    <button
      type="button"
      class="justify-self-start rounded px-1 py-0.5 text-[12px] text-fg-3 hover:bg-hover hover:text-fg"
      aria-expanded={showProcess}
      onclick={() => (showProcess = !showProcess)}
    >
      {showProcess ? "▾ Hide process" : "▸ View process"}
    </button>
  {/if}

  {#if showProcess && card.groups.length > 0}
    <div class="grid max-h-[340px] gap-2.5 overflow-auto" data-testid="review-process">
      {#each card.groups as group (group.stage)}
        <div>
          <div class="mb-1 text-[10.5px] font-semibold uppercase tracking-[.06em] text-muted">
            {group.label}{#if group.note}<span class="font-normal normal-case tracking-normal"> · {group.note}</span>{/if}
          </div>
          {#each group.rows as row, i (row.task.id)}
            {@const t = row.task}
            {@const bgId = t.background_task_id}
            <div class="grid grid-cols-[10px_minmax(0,1fr)] gap-2 py-1.5 {i > 0 ? 'border-t border-line' : ''}" data-status={t.status}>
              <span class="mt-[5px] inline-block h-[7px] w-[7px] rounded-full" style:background={row.dotColor}></span>
              <div class="min-w-0">
                <div class="flex items-center gap-2 text-[12.5px]">
                  <b class="truncate font-semibold">{row.title}</b>
                  {#if row.round}<span class="font-mono text-[10.5px] text-muted">{row.round}</span>{/if}
                  <span class="ml-auto inline-flex shrink-0 items-center gap-[5px] rounded-[5px] px-[7px] text-[10.5px] font-semibold leading-[18px] {pillClass[t.status]}">
                    {#if t.status === "running"}
                      <i class="inline-block h-2 w-2 rounded-full border-[1.5px] border-question border-r-transparent motion-safe:animate-spin"></i>
                    {/if}
                    {row.statusLabel}
                  </span>
                </div>
                <div class="truncate text-[11.5px] text-muted">{[t.purpose, row.model].filter(Boolean).join(" · ")}</div>
                {#if t.status === "running"}
                  <div class="mt-[5px] h-1 overflow-hidden rounded-sm bg-ink-600" aria-hidden="true">
                    <i class="block h-full w-1/3 rounded-sm motion-safe:animate-[run-indeterminate_1.4s_ease-in-out_infinite]" style:background={row.dotColor}></i>
                  </div>
                {/if}
                {#if t.status === "failed"}
                  <div class="mt-[3px] flex items-center gap-1.5 text-[11.5px] text-del-fg">
                    <span class="min-w-0 [overflow-wrap:anywhere]">{t.error ?? "Failed without a reason"}</span>
                    {#if card.canRestart}
                      <button type="button" class="shrink-0 rounded px-1.5 text-fg-2 hover:bg-hover" title="Start the review again" onclick={restart}>Retry</button>
                    {/if}
                  </div>
                {/if}
                {#if bgId && t.status !== "waiting" && t.status !== "queued"}
                  <button
                    type="button"
                    class="mt-1 rounded px-1 text-[11px] text-fg-3 hover:bg-hover hover:text-fg"
                    aria-expanded={!!openLogs[t.id]}
                    onclick={() => toggleLog(t.id)}
                  >
                    {openLogs[t.id] ? "Hide log" : "Live log"}
                  </button>
                  {#if openLogs[t.id]}
                    <RunningAgentPanel taskId={bgId} task={tasks.find((x) => x.id === bgId) ?? null} />
                  {/if}
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  @keyframes -global-run-indeterminate {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }
</style>
