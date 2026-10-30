<script lang="ts">
  import type { BackgroundTaskSnapshot, AgentLogEntry } from "$lib/types";
  import { sourceColor } from "$lib/utils/agentLog";
  import { copyToClipboard } from "$lib/clipboard";
  import { invoke } from "@tauri-apps/api/core";

  /** Live log tail of one background task, expanded inside a jobs-popover row. */
  interface Props {
    taskId: string;
    /** The task's snapshot row; absent once the backend pruned it. */
    task?: BackgroundTaskSnapshot | null;
  }

  const { taskId, task = null }: Props = $props();

  // The snapshot tail is ~40 lines and only refreshes with the snapshot, so it
  // seeds the view and the poll below keeps it live.
  let logLines = $derived<AgentLogEntry[]>(task?.recent_log ?? []);
  let logContainer = $state<HTMLDivElement | null>(null);
  let stickToBottom = $state(true);
  const running = $derived(task?.status === "running");

  $effect(() => {
    if (!running) return;
    const id = taskId;
    let cancelled = false;
    const handle = setInterval(async () => {
      try {
        const result = await invoke<AgentLogEntry[]>("get_background_task_log", { taskId: id });
        if (!cancelled) logLines = result;
      } catch {
        // A pruned task stops answering; the snapshot tail stays on screen.
      }
    }, 1000);
    return () => {
      cancelled = true;
      clearInterval(handle);
    };
  });

  function handleScroll() {
    if (!logContainer) return;
    const distance = logContainer.scrollHeight - logContainer.scrollTop - logContainer.clientHeight;
    stickToBottom = distance < 8;
  }

  $effect(() => {
    void logLines.length;
    if (logContainer && stickToBottom) logContainer.scrollTo({ top: logContainer.scrollHeight });
  });
</script>

<div
  bind:this={logContainer}
  onscroll={handleScroll}
  class="mt-1.5 max-h-[170px] overflow-y-auto rounded-md border border-line bg-bg px-2 py-1.5 grid gap-0.5"
  data-testid="running-agent-log"
>
  {#if logLines.length > 0}
    {#each logLines as entry, i (i)}
      <div class="font-mono text-[10.5px] leading-[1.5] [overflow-wrap:anywhere] {sourceColor(entry.source)}">{entry.text}</div>
    {/each}
  {:else if running}
    <div class="text-[11px] text-muted italic">Waiting for output…</div>
  {:else}
    <div class="text-[11px] text-muted">No output recorded.</div>
  {/if}
  {#if task?.debug_log_path && !running}
    {@const logPath = task.debug_log_path}
    <button
      type="button"
      class="justify-self-start mt-1 text-[10.5px] font-mono text-accent hover:underline"
      onclick={() => void copyToClipboard(logPath)}
    >
      Copy log path
    </button>
  {/if}
</div>
