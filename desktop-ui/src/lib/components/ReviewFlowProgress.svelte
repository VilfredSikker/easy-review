<script lang="ts">
  import type { ReviewFlowSnapshot, ReviewFlowTaskProgress } from "$lib/types";
  import { reviewFlowProgressView, reviewFlowStage } from "$lib/reviewFlow";

  interface Props {
    flow: ReviewFlowSnapshot;
  }

  const { flow }: Props = $props();
  const progress = $derived(reviewFlowProgressView(flow));
  const tasks = $derived(flow.tasks ?? []);

  function statusLabel(status: ReviewFlowTaskProgress["status"]): string {
    return ({
      waiting: "Waiting",
      queued: "Queued",
      running: "Running",
      done: "Done",
      failed: "Failed",
      cancelled: "Cancelled",
      skipped: "Skipped",
    })[status];
  }

  function statusClass(status: ReviewFlowTaskProgress["status"]): string {
    return ({
      waiting: "border-[var(--arena-border)] text-[var(--arena-fg-subtle)]",
      queued: "border-warning/30 bg-warning/10 text-warning",
      running: "border-[var(--arena-periwinkle)]/40 bg-[var(--arena-periwinkle)]/10 text-[var(--arena-periwinkle)]",
      done: "border-success/30 bg-success/10 text-success",
      failed: "border-risk-high/30 bg-risk-high/10 text-risk-high",
      cancelled: "border-[var(--arena-border)] text-[var(--arena-fg-subtle)]",
      skipped: "border-[var(--arena-border)] text-[var(--arena-fg-subtle)]",
    })[status];
  }

  function modelIdentity(task: ReviewFlowTaskProgress): string {
    const provider = task.provider_label && task.provider_label !== task.provider_id
      ? `${task.provider_label} (${task.provider_id})`
      : task.provider_id;
    const model = task.model_label && task.model_label !== task.model_id
      ? `${task.model_label} (${task.model_id})`
      : task.model_id;
    return `${provider} · ${model}`;
  }

  const stageBarClasses = {
    waiting: "bg-[var(--arena-bg-3)]",
    active: "bg-[color-mix(in_srgb,var(--arena-periwinkle)_45%,var(--arena-bg-3))]",
    done: "bg-[var(--arena-periwinkle)]",
    failed: "bg-risk-high",
    skipped: "bg-[var(--arena-bg-3)]",
  };
</script>

<section
  class="rounded-lg border border-[var(--arena-border)] bg-[var(--arena-bg-0)] p-2.5"
  aria-label="Review process progress"
  data-testid="review-flow-progress"
>
  <div class="flex items-start justify-between gap-2">
    <div class="min-w-0">
      <p class="text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-periwinkle)]">
        {progress?.headerLabel ?? "Review process"}
      </p>
      <p class="mt-0.5 text-[11px] text-[var(--arena-fg-subtle)]">
        {progress?.activeGoal ?? reviewFlowStage(flow)}
      </p>
    </div>
    {#if progress?.showCounts}
      <span class="shrink-0 text-[10px] text-[var(--arena-fg-muted)]">
        {progress.finished} finished · {progress.remaining} remaining
      </span>
    {/if}
  </div>

  {#if progress}
    <ol class="mt-2 flex gap-1" aria-label="Review stages">
      {#each progress.rail as stage (stage.stage)}
        <li class="min-w-0 flex-1" aria-label="{stage.label}: {stage.state}">
          <div
            class="h-1 rounded-full {stageBarClasses[stage.state]}"
          ></div>
          <span class="mt-1 block truncate text-[9px] {stage.state === 'active' ? 'text-[var(--arena-periwinkle)]' : 'text-[var(--arena-fg-subtle)]'}">
            {stage.label}{#if stage.state === "skipped"} · skipped{/if}
          </span>
        </li>
      {/each}
    </ol>

    {#if progress.visibleTasks.length > 0}
      <ul class="mt-2 max-h-24 space-y-1 overflow-y-auto" aria-label="Active and failed review tasks">
        {#each progress.visibleTasks as task (task.id)}
          <li class="rounded border border-[var(--arena-border)] bg-[var(--arena-bg-1)] px-2 py-1.5">
            <div class="flex min-w-0 items-center gap-2">
              <span class="min-w-0 flex-1 truncate text-[10px] text-[var(--arena-fg)]">{task.label}</span>
              <span class="shrink-0 rounded border px-1.5 py-0.5 text-[9px] {statusClass(task.status)}">
                {statusLabel(task.status)}
              </span>
            </div>
            <p class="mt-0.5 truncate text-[9px] text-[var(--arena-fg-subtle)]">{modelIdentity(task)}</p>
            {#if task.error}
              <p class="mt-1 break-words text-[10px] text-risk-high">{task.error}</p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <details class="mt-2 border-t border-[var(--arena-border)] pt-1.5">
      <summary class="cursor-pointer text-[10px] font-medium text-[var(--arena-fg-muted)] hover:text-[var(--arena-fg)]">
        View process{#if progress.showCounts} · {progress.finished} finished, {progress.remaining} remaining{/if}
      </summary>
      <ul class="mt-2 max-h-56 space-y-1.5 overflow-y-auto" aria-label="Captured review process">
        {#each tasks as task (task.id)}
          <li class="rounded border border-[var(--arena-border)] bg-[var(--arena-bg-1)] p-2">
            <div class="flex items-center gap-2">
              <span class="min-w-0 flex-1 truncate text-[10px] font-medium text-[var(--arena-fg)]">{task.label}</span>
              <span class="shrink-0 rounded border px-1.5 py-0.5 text-[9px] {statusClass(task.status)}">
                {statusLabel(task.status)}
              </span>
            </div>
            <p class="mt-1 text-[10px] leading-relaxed text-[var(--arena-fg-subtle)]">{task.purpose}</p>
            <p class="mt-1 break-all font-mono text-[9px] text-[var(--arena-fg-muted)]">{modelIdentity(task)}</p>
            {#if task.round != null}
              <p class="mt-0.5 text-[9px] text-[var(--arena-fg-muted)]">
                Round {task.round}{task.total_rounds != null ? ` of ${task.total_rounds}` : ""}
              </p>
            {/if}
            {#if task.error}
              <p class="mt-1 break-words text-[10px] text-risk-high">{task.error}</p>
            {/if}
          </li>
        {/each}
      </ul>
    </details>
  {:else}
    <p class="mt-2 text-[10px] text-[var(--arena-fg-muted)]">
      Process details are not available for this review.
    </p>
  {/if}
</section>
