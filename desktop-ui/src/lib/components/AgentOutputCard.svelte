<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { AgentLogEntry, AgentCommandStatus } from "$lib/types";
  import { sourceColor } from "$lib/utils/agentLog";

  const commands = $derived<AgentCommandStatus[]>(app.snapshot?.agent_commands ?? []);
  const log = $derived<AgentLogEntry[]>(app.snapshot?.agent_log ?? []);

  const hasActivity = $derived(commands.length > 0 || log.length > 0);
  const isRunning = $derived(commands.some((c) => c.status === "running"));

  function statusColor(status: string): string {
    if (status === "running") return "text-accent";
    if (status === "done") return "text-add-fg";
    if (status === "failed") return "text-del-fg";
    return "text-ink-300";
  }
</script>

{#if hasActivity}
  <div class="rounded-lg border border-hairline bg-card overflow-hidden flex flex-col max-h-[360px]">
    <!-- header -->
    <div class="flex items-center gap-2 px-3 py-2 border-b border-hairline shrink-0">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent shrink-0">
        <path d="M12 2L2 7l10 5 10-5-10-5z"/><path d="M2 17l10 5 10-5"/><path d="M2 12l10 5 10-5"/>
      </svg>
      <span class="text-xs font-medium text-ink-200">Agent Output</span>
      {#if isRunning}
        <span class="ml-auto flex items-center gap-1 text-[10px] text-accent font-mono">
          <span class="w-1.5 h-1.5 rounded-full bg-accent animate-pulse"></span>
          running
        </span>
      {/if}
    </div>

    <!-- command statuses -->
    {#if commands.length > 0}
      <div class="px-3 py-2 space-y-1 border-b border-hairline shrink-0">
        {#each commands as cmd}
          <div class="flex items-center gap-2 text-xs font-mono">
            <span class="text-ink-400">{cmd.name}</span>
            <span class="{statusColor(cmd.status)} ml-auto">{cmd.status}</span>
          </div>
          {#if cmd.error}
            <div class="text-[10px] text-del-fg/80 font-mono leading-relaxed break-all">{cmd.error}</div>
          {/if}
        {/each}
      </div>
    {/if}

    <!-- log output -->
    {#if log.length > 0}
      <div class="flex-1 min-h-0 overflow-y-auto px-3 py-2 space-y-0.5">
        {#each log as entry}
          <div class="text-[10px] font-mono leading-relaxed break-all {sourceColor(entry.source)}">
            {entry.text}
          </div>
        {/each}
      </div>
    {:else if commands.length > 0 && isRunning}
      <div class="px-3 py-3 text-[10px] text-ink-400 italic">Waiting for output…</div>
    {/if}
  </div>
{/if}
