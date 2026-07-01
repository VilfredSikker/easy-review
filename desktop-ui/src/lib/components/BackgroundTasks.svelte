<script lang="ts">
  import type { BackgroundTaskSnapshot } from "$lib/types";
  import RunningAgentPanel from "./RunningAgentPanel.svelte";
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    tasks: BackgroundTaskSnapshot[];
    avoidRightPanel?: boolean;
    rightPanelWidth?: number;
  }
  const { tasks, avoidRightPanel = false, rightPanelWidth = 340 }: Props = $props();

  let expandedTaskId = $state<string | null>(null);

  const DISPLAY_WINDOW_MS = 8_000;
  const LABEL_MAX = 32;

  let now = $state(Date.now());
  $effect(() => {
    const handle = setInterval(() => {
      now = Date.now();
    }, 1_000);
    return () => clearInterval(handle);
  });

  const visible = $derived.by(() => {
    const out: BackgroundTaskSnapshot[] = [];
    for (const t of tasks) {
      if (t.status === "running" || t.status === "queued") {
        out.push(t);
        continue;
      }
      const finished = t.finished_at_ms ?? 0;
      if (finished > 0 && now - finished <= DISPLAY_WINDOW_MS) {
        out.push(t);
      }
    }
    return out;
  });

  const running = $derived(visible.filter((t) => t.status === "running"));
  const queued = $derived(visible.filter((t) => t.status === "queued"));
  const failed = $derived(visible.filter((t) => t.status === "failed"));
  const done = $derived(visible.filter((t) => t.status === "done"));

  function truncate(s: string): string {
    if (!s) return "";
    return s.length > LABEL_MAX ? s.slice(0, LABEL_MAX - 1) + "…" : s;
  }

  type Pill = {
    key: string;
    status: "queued" | "running" | "done" | "failed";
    text: string;
    title?: string;
    /** Queued pills can be cancelled — removes the task from the queue. */
    cancelTaskId?: string;
  };

  const pills = $derived.by<Pill[]>(() => {
    const out: Pill[] = [];
    if (running.length === 1) {
      const t = running[0];
      out.push({
        key: `run-${t.id}`,
        status: "running",
        text: t.target_label
          ? `${t.label} running · ${truncate(t.target_label)}`
          : `${t.label} running`,
      });
    } else if (running.length > 1) {
      out.push({
        key: "run-multi",
        status: "running",
        text: `${running.length} reviews running`,
        title: running
          .map((t) => (t.target_label ? `${t.label} · ${t.target_label}` : t.label))
          .join(", "),
      });
    }
    if (queued.length === 1) {
      const t = queued[0];
      out.push({
        key: `queue-${t.id}`,
        status: "queued",
        text: t.target_label
          ? `${t.label} queued · ${truncate(t.target_label)}`
          : `${t.label} queued`,
        title: "Waiting for a free review slot — click ✕ to remove",
        cancelTaskId: t.id,
      });
    } else if (queued.length > 1) {
      out.push({
        key: "queue-multi",
        status: "queued",
        text: `${queued.length} reviews queued`,
        title: queued
          .map((t) => (t.target_label ? `${t.label} · ${t.target_label}` : t.label))
          .join(", "),
      });
    }
    for (const t of failed) {
      out.push({
        key: `fail-${t.id}`,
        status: "failed",
        text: t.target_label
          ? `${t.label} failed · ${truncate(t.target_label)}`
          : `${t.label} failed`,
        title: t.error ?? undefined,
      });
    }
    for (const t of done) {
      out.push({
        key: `done-${t.id}`,
        status: "done",
        text: t.target_label
          ? `${t.label} done · ${truncate(t.target_label)}`
          : `${t.label} done`,
      });
    }
    return out;
  });

  function dotColor(status: Pill["status"]): string {
    if (status === "running") return "bg-accent animate-pulse";
    if (status === "queued") return "bg-warning";
    if (status === "done") return "bg-add-fg";
    return "bg-del-fg";
  }

  function cancelQueued(taskId: string, event: MouseEvent) {
    event.stopPropagation();
    void app.cmd("cancel_queued_review", { id: taskId });
  }

  const rightOffset = $derived(avoidRightPanel ? rightPanelWidth + 16 : 24);
</script>

{#if pills.length > 0}
  <div
    class="fixed bottom-10 flex flex-col items-end gap-1 z-50"
    style="right: {rightOffset}px"
    aria-label="Background tasks"
  >
    {#if expandedTaskId !== null && visible.length > 0}
      <RunningAgentPanel
        tasks={visible}
        onClose={() => (expandedTaskId = null)}
        {avoidRightPanel}
        {rightPanelWidth}
      />
    {/if}

    {#each pills as pill (pill.key)}
      {@const pillTaskId = pill.key.startsWith("run-")
        ? running[0]?.id
        : pill.key.startsWith("queue-")
          ? queued[0]?.id
          : pill.key.startsWith("fail-")
            ? pill.key.slice("fail-".length)
            : pill.key.slice("done-".length)}
      <div
        class="bg-ink-800 text-ink-100 text-[11px] font-mono rounded-sm border border-ink-500 shadow flex items-center max-w-[280px]"
      >
        <button
          class="px-2.5 py-1 flex items-center gap-1.5 min-w-0 cursor-pointer hover:bg-ink-700 transition-colors rounded-sm"
          title={pill.title ?? ""}
          onclick={() => {
            expandedTaskId = expandedTaskId === pillTaskId ? null : (pillTaskId ?? null);
          }}
        >
          <span
            class="inline-block w-1.5 h-1.5 rounded-full shrink-0 {dotColor(pill.status)}"
            aria-hidden="true"
          ></span>
          <span class="truncate">{pill.text}</span>
        </button>
        {#if pill.cancelTaskId}
          <button
            class="px-1.5 py-1 shrink-0 text-ink-300 hover:text-ink-100 hover:bg-ink-700 transition-colors rounded-sm cursor-pointer"
            title="Remove from queue"
            aria-label="Remove queued review"
            onclick={(e) => cancelQueued(pill.cancelTaskId!, e)}
          >
            ✕
          </button>
        {/if}
      </div>
    {/each}
  </div>
{/if}
