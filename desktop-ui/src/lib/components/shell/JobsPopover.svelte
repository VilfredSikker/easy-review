<script lang="ts">
  import type { BackgroundTaskSnapshot, ReviewFlowSnapshot } from "$lib/types";
  import type { RunningJob } from "$lib/runningJobs";
  import { app } from "$lib/stores/app.svelte";
  import { arena } from "$lib/stores/arena.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import RunningAgentPanel from "$lib/components/RunningAgentPanel.svelte";
  import ReviewRunCard from "./ReviewRunCard.svelte";

  /** "AI in progress": the review flow as one card, then every other job. */
  interface Props {
    open: boolean;
    onClose: () => void;
    /** Viewport position of the popover's top-right corner (under the pill). */
    anchor: { top: number; right: number } | null;
    flow: ReviewFlowSnapshot | null;
    /** Jobs outside the review flow (see `otherJobs`). */
    jobs: readonly RunningJob[];
    tasks: readonly BackgroundTaskSnapshot[];
    erDir: string | null;
    mode?: string;
    nowMs: number;
  }

  const { open, onClose, anchor, flow, jobs, tasks, erDir, mode, nowMs }: Props = $props();

  const WIDTH = 420;
  const EDGE = 8;
  const active = $derived(jobs.filter((j) => j.status === "running" || j.status === "queued").length);
  let openLogs = $state<Record<string, boolean>>({});

  const panelStyle = $derived.by(() => {
    const top = anchor?.top ?? 48;
    const vw = typeof window === "undefined" ? WIDTH + 2 * EDGE : window.innerWidth;
    const left = Math.max(EDGE, Math.min((anchor?.right ?? vw - EDGE) - WIDTH, vw - WIDTH - EDGE));
    return `top: ${top}px; left: ${left}px; width: min(${WIDTH}px, calc(100vw - ${2 * EDGE}px)); max-height: calc(100vh - ${top + 16}px)`;
  });

  function elapsed(job: RunningJob): string {
    if (job.status === "done") return "done";
    if (job.status === "failed") return "failed";
    if (job.status === "queued") return "queued";
    if (job.startedAtMs == null) return "";
    const s = Math.max(0, Math.floor((nowMs - job.startedAtMs) / 1000));
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60}s`;
  }

  /** `task:<id>` → the background task id the cancel and log commands take. */
  function taskId(job: RunningJob): string | null {
    return job.source === "background_task" ? job.id.slice("task:".length) : null;
  }

  function cancel(job: RunningJob) {
    const id = taskId(job);
    if (!id) return;
    void app.cmd(job.status === "queued" ? "cancel_queued_review" : "cancel_running_review", { id });
  }

  function openOutput(job: RunningJob) {
    onClose();
    if (job.source === "arena") arena.showRunningProgress();
    else app.setMainView("agent-output");
  }

  function openSettings() {
    onClose();
    app.openSettings("concurrency");
  }

  const dot: Record<RunningJob["status"], string> = {
    running: "bg-accent motion-safe:animate-pulse",
    queued: "bg-warning",
    done: "bg-add-fg",
    failed: "bg-del-fg",
  };
</script>

<ModalShell
  {open}
  ariaLabel="AI in progress"
  {onClose}
  backdropClass="fixed inset-0 z-[39]"
  panelClass="fixed z-[40] flex flex-col overflow-auto rounded-xl border border-hairline bg-card shadow-[0_18px_44px_rgba(0,0,0,.32)] outline-none"
  {panelStyle}
  dataModal="jobs-popover"
>
  {#if flow}
    <ReviewRunCard {flow} {tasks} {erDir} {mode} onNavigate={onClose} />
  {/if}

  {#if jobs.length > 0 || !flow}
    <div class="grid gap-px px-3.5 py-3 border-b border-line {flow ? 'border-t' : ''}">
      <b class="text-[13px] font-semibold">{flow ? "Other AI work" : "AI in progress"}</b>
      <span class="text-[11.5px] text-muted">{active} running</span>
    </div>
    {#each jobs as job (job.id)}
      {@const bgId = taskId(job)}
      <div class="grid gap-[7px] border-b border-line px-3.5 py-3 {job.status === 'done' ? 'opacity-70' : ''}" data-testid="job-row">
        <div class="flex items-center gap-2 text-[12.5px]">
          <span class="inline-block h-1.5 w-1.5 shrink-0 rounded-full {dot[job.status]}" aria-hidden="true"></span>
          <b class="min-w-0 truncate font-semibold">{job.label}</b>
          <span class="ml-auto shrink-0 font-mono text-[11px] text-muted">{elapsed(job)}</span>
        </div>
        {#if job.error}<div class="text-[11.5px] text-del-fg [overflow-wrap:anywhere]">{job.error}</div>{/if}
        {#if bgId && openLogs[job.id]}
          <RunningAgentPanel taskId={bgId} task={tasks.find((t) => t.id === bgId) ?? null} />
        {/if}
        <div class="-mb-1 -mr-1.5 flex flex-wrap items-center gap-0.5 text-[11.5px]">
          <span class="flex-1"></span>
          {#if bgId}
            <button
              type="button"
              class="rounded px-1.5 py-0.5 text-fg-3 hover:bg-hover hover:text-fg"
              aria-expanded={!!openLogs[job.id]}
              onclick={() => (openLogs = { ...openLogs, [job.id]: !openLogs[job.id] })}
            >
              {openLogs[job.id] ? "Hide log" : "Live log"}
            </button>
          {:else}
            <button type="button" class="rounded px-1.5 py-0.5 text-fg-3 hover:bg-hover hover:text-fg" onclick={() => openOutput(job)}>
              {job.source === "arena" ? "View progress" : "Open output"}
            </button>
          {/if}
          {#if bgId && (job.status === "running" || job.status === "queued")}
            <button type="button" class="rounded px-1.5 py-0.5 text-del-fg hover:bg-hover" onclick={() => cancel(job)}>
              {job.status === "queued" ? "Remove" : "Cancel"}
            </button>
          {/if}
        </div>
      </div>
    {:else}
      <div class="px-4 py-4 text-[12.5px] text-muted">Nothing running. Start a review from ⌘K.</div>
    {/each}
  {/if}

  <div class="flex items-center justify-between py-2 pl-3.5 pr-2.5 text-[11.5px]">
    <span class="text-muted">Queued runs wait for a free review slot</span>
    <button type="button" class="rounded px-1.5 py-0.5 text-fg-3 hover:bg-hover hover:text-fg" onclick={openSettings}>Concurrency…</button>
  </div>
</ModalShell>
