<script lang="ts">
  import type { BackgroundTaskSnapshot } from "$lib/types";
  import RunningAgentPanel from "./RunningAgentPanel.svelte";

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
      if (t.status === "running") {
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
  const failed = $derived(visible.filter((t) => t.status === "failed"));
  const done = $derived(visible.filter((t) => t.status === "done"));

  function truncate(s: string): string {
    if (!s) return "";
    return s.length > LABEL_MAX ? s.slice(0, LABEL_MAX - 1) + "…" : s;
  }

  type Pill = {
    key: string;
    status: "running" | "done" | "failed";
    text: string;
    title?: string;
  };

  const pills = $derived.by<Pill[]>(() => {
    const out: Pill[] = [];
    if (running.length === 1) {
      out.push({
        key: `run-${running[0].id}`,
        status: "running",
        text: `Review running · ${truncate(running[0].label)}`,
      });
    } else if (running.length > 1) {
      out.push({
        key: "run-multi",
        status: "running",
        text: `${running.length} reviews running`,
        title: running.map((t) => t.label).join(", "),
      });
    }
    for (const t of failed) {
      out.push({
        key: `fail-${t.id}`,
        status: "failed",
        text: `Review failed · ${truncate(t.label)}`,
        title: t.error ?? undefined,
      });
    }
    for (const t of done) {
      out.push({
        key: `done-${t.id}`,
        status: "done",
        text: `Review done · ${truncate(t.label)}`,
      });
    }
    return out;
  });

  function dotColor(status: Pill["status"]): string {
    if (status === "running") return "bg-accent animate-pulse";
    if (status === "done") return "bg-add-fg";
    return "bg-del-fg";
  }

  const rightOffset = $derived(avoidRightPanel ? rightPanelWidth + 16 : 24);
</script>

{#if pills.length > 0}
  <div
    class="fixed bottom-10 flex flex-col items-end gap-1 z-40"
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
      {@const pillTaskId = pill.key.startsWith("run-multi")
        ? running[0]?.id
        : pill.key.startsWith("run-")
          ? running[0]?.id
          : pill.key.startsWith("fail-")
            ? pill.key.slice("fail-".length)
            : pill.key.slice("done-".length)}
      <button
        class="bg-ink-800/90 text-ink-100 text-[11px] font-mono px-2.5 py-1 rounded-sm border border-ink-500/40 shadow flex items-center gap-1.5 max-w-[260px] cursor-pointer hover:bg-ink-700/90 transition-colors"
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
    {/each}
  </div>
{/if}
