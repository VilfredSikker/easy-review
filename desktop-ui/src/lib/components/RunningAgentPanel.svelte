<script lang="ts">
  import type { BackgroundTaskSnapshot, AgentLogEntry } from "$lib/types";
  import { sourceColor } from "$lib/utils/agentLog";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { invoke } from "@tauri-apps/api/core";

  interface Props {
    tasks: BackgroundTaskSnapshot[];
    onClose: () => void;
    avoidRightPanel?: boolean;
    rightPanelWidth?: number;
  }

  const { tasks, onClose, avoidRightPanel = false, rightPanelWidth = 340 }: Props = $props();

  const LABEL_MAX = 20;

  let activeTaskId = $state(tasks[0]?.id ?? "");
  let logLines = $state<AgentLogEntry[]>([]);
  let logContainer = $state<HTMLDivElement | null>(null);
  let stickToBottom = $state(true);

  function handleScroll() {
    if (!logContainer) return;
    const distance = logContainer.scrollHeight - logContainer.scrollTop - logContainer.clientHeight;
    stickToBottom = distance < 8;
  }

  const rightOffset = $derived(avoidRightPanel ? rightPanelWidth + 16 : 24);

  const activeTask = $derived(tasks.find((t) => t.id === activeTaskId) ?? tasks[0] ?? null);

  // Keep activeTaskId valid when task list changes
  $effect(() => {
    const ids = tasks.map((t) => t.id);
    if (!ids.includes(activeTaskId)) {
      activeTaskId = tasks[0]?.id ?? "";
    }
  });

  // Seed logLines from snapshot whenever the active task changes
  $effect(() => {
    const task = tasks.find((t) => t.id === activeTaskId);
    logLines = task?.recent_log ?? [];
    stickToBottom = true;
  });

  // Poll for live log when active task is running
  $effect(() => {
    const id = activeTaskId;
    const task = tasks.find((t) => t.id === id);
    if (!task || task.status !== "running") return;

    const handle = setInterval(async () => {
      try {
        const result = await invoke<AgentLogEntry[]>("get_background_task_log", { taskId: id });
        if (activeTaskId === id) logLines = result;
      } catch {
        // ignore poll errors
      }
    }, 1000);

    return () => clearInterval(handle);
  });

  // Auto-scroll to bottom when logLines changes — only if user is pinned at bottom
  $effect(() => {
    void logLines.length;
    if (logContainer && stickToBottom) {
      logContainer.scrollTo({ top: logContainer.scrollHeight });
    }
  });

  // Timer for elapsed time
  let now = $state(Date.now());
  $effect(() => {
    const handle = setInterval(() => {
      now = Date.now();
    }, 1000);
    return () => clearInterval(handle);
  });

  function elapsedText(startMs: number): string {
    const secs = Math.floor((now - startMs) / 1000);
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    const rem = secs % 60;
    return `${mins}m${rem}s`;
  }

  function truncate(s: string): string {
    if (!s) return "";
    return s.length > LABEL_MAX ? s.slice(0, LABEL_MAX - 1) + "…" : s;
  }

  function dotClass(status: string): string {
    if (status === "running") return "bg-accent animate-pulse";
    if (status === "done") return "bg-add-fg";
    return "bg-del-fg";
  }

  async function copyLogPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
    } catch {
      // ignore
    }
  }
</script>

<ModalShell
  open={tasks.length > 0}
  ariaLabel="Running agent output"
  {onClose}
  backdropClass="fixed inset-0 z-[39]"
  panelClass="fixed z-[40] w-[360px] max-h-[260px] flex flex-col bg-ink-900/95 border border-ink-500/40 rounded-lg shadow-xl overflow-hidden outline-none"
  panelStyle="bottom: 64px; right: {rightOffset}px"
>
  <!-- Tab strip -->
  <div class="flex items-center gap-0.5 px-1.5 pt-1.5 pb-0 shrink-0 overflow-x-auto">
    {#each tasks as task (task.id)}
      <button
        class="flex items-center gap-1 px-2 py-1 rounded-t text-[10px] font-mono shrink-0 transition-colors
          {activeTaskId === task.id ? 'bg-ink-700/60 text-ink-100' : 'text-ink-400 hover:text-ink-200 hover:bg-ink-800/60'}"
        onclick={() => { activeTaskId = task.id; }}
      >
        <span class="inline-block w-1.5 h-1.5 rounded-full shrink-0 {dotClass(task.status)}" aria-hidden="true"></span>
        <span>{truncate(task.label)}</span>
        {#if task.scope}
          <span class="text-[9px] font-mono text-ink-400 bg-ink-800/80 px-0.5 rounded">{task.scope}</span>
        {/if}
      </button>
    {/each}
  </div>

  <!-- Divider -->
  <div class="border-t border-ink-500/40 shrink-0"></div>

  {#if activeTask}
    <!-- Task header -->
    <div class="flex items-center gap-2 px-3 py-1.5 shrink-0">
      <span class="text-[10px] font-mono text-ink-200 truncate flex-1">
        {activeTask.label}{#if activeTask.target_label}<span class="text-ink-400"> · {activeTask.target_label}</span>{/if}
      </span>
      {#if activeTask.status === "failed" && activeTask.error}
        <span class="text-[10px] font-mono text-del-fg shrink-0 truncate max-w-[120px]">{activeTask.error}</span>
      {:else}
        <span class="text-[10px] font-mono text-ink-400 shrink-0">{elapsedText(activeTask.started_at_ms)}</span>
      {/if}
    </div>

    <!-- Log area -->
    <div
      bind:this={logContainer}
      onscroll={handleScroll}
      class="flex-1 min-h-0 overflow-y-auto px-3 py-2 space-y-0.5"
    >
      {#if logLines.length > 0}
        {#each logLines as entry}
          <div class="text-[10px] font-mono leading-relaxed break-all {sourceColor(entry.source)}">{entry.text}</div>
        {/each}
      {:else if activeTask.status === "running"}
        <div class="text-[10px] text-ink-400 italic">Waiting for output…</div>
      {/if}

      {#if activeTask.debug_log_path && (activeTask.status === "done" || activeTask.status === "failed")}
        <button
          class="text-[10px] text-accent font-mono mt-1 hover:underline"
          onclick={() => copyLogPath(activeTask!.debug_log_path!)}
        >
          Copy log path
        </button>
      {/if}
    </div>
  {/if}
</ModalShell>
