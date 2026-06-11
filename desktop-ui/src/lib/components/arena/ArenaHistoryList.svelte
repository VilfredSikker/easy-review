<script lang="ts">
  import type { ArenaRunSummary, RunStatus } from "$lib/types/arena";
  import { isArenaRunActive } from "$lib/stores/arena.svelte";

  interface Props {
    summaries: ArenaRunSummary[];
    activeRunId?: string | null;
    onOpen: (runId: string) => void;
    onDelete: (runId: string) => void;
  }

  const { summaries, activeRunId = null, onOpen, onDelete }: Props = $props();

  function statusLabel(status: RunStatus): string {
    if (status === "queued") return "Queued";
    if (status === "complete") return "Complete";
    if (status === "failed") return "Failed";
    if (status === "cancelled") return "Cancelled";
    if (typeof status === "object" && status !== null && "running" in status) {
      return `Round ${status.running.round}`;
    }
    return "—";
  }

  function statusClass(status: RunStatus): string {
    if (status === "complete") return "text-[var(--arena-ok)]";
    if (status === "failed") return "text-[var(--arena-err)]";
    if (status === "cancelled") return "text-[var(--arena-fg-subtle)]";
    if (isArenaRunActive(status)) return "text-[var(--arena-periwinkle)]";
    return "text-[var(--arena-fg-muted)]";
  }

  function formatWhen(iso: string): string {
    const t = Date.parse(iso);
    if (Number.isNaN(t)) return iso;
    const diff = Date.now() - t;
    const min = Math.floor(diff / 60_000);
    if (min < 1) return "just now";
    if (min < 60) return `${min}m ago`;
    const hr = Math.floor(min / 60);
    if (hr < 24) return `${hr}h ago`;
    const day = Math.floor(hr / 24);
    return `${day}d ago`;
  }

  function shortId(id: string): string {
    return id.length > 20 ? `${id.slice(0, 18)}…` : id;
  }
</script>

{#if summaries.length === 0}
  <p class="mt-2 text-[11px] text-[var(--arena-fg-muted)]">No arena runs on this branch yet.</p>
{:else}
  <p
    class="mt-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--arena-fg-faint)]"
  >
    History on this branch
  </p>
  <ul
    class="mt-1.5 max-h-[200px] space-y-1 overflow-y-auto rounded-md border border-[var(--arena-border)] bg-[var(--arena-bg-0)] p-1"
    role="list"
  >
    {#each summaries as run (run.id)}
      {@const active = activeRunId === run.id && isArenaRunActive(run.status)}
      <li>
        <div
          class="group flex items-center gap-1 rounded-md px-2 py-1.5 transition-colors
            {active ? 'bg-[var(--arena-bg-3)] ring-1 ring-[var(--arena-periwinkle)]/40' : 'hover:bg-[var(--arena-bg-2)]'}"
        >
          <button
            type="button"
            class="min-w-0 flex-1 text-left"
            onclick={() => onOpen(run.id)}
          >
            <p class="truncate text-[11px] font-medium text-[var(--arena-fg)]">
              {run.title ?? shortId(run.id)}
            </p>
            <p class="text-[10px] text-[var(--arena-fg-subtle)]">
              <span class={statusClass(run.status)}>{statusLabel(run.status)}</span>
              · {formatWhen(run.created_at)}
              · {run.finding_count} findings
              · {run.reviewer_count} reviewer{run.reviewer_count === 1 ? "" : "s"}
            </p>
          </button>
          <button
            type="button"
            class="shrink-0 rounded p-1 text-[var(--arena-fg-faint)] opacity-0 transition-opacity hover:bg-[var(--arena-bg-3)] hover:text-[var(--arena-err)] group-hover:opacity-100 disabled:cursor-not-allowed disabled:opacity-30"
            title={isArenaRunActive(run.status) ? "Cancel the run before deleting" : "Delete run"}
            disabled={isArenaRunActive(run.status)}
            aria-label="Delete run"
            onclick={(e) => {
              e.stopPropagation();
              onDelete(run.id);
            }}
          >
            ✕
          </button>
        </div>
      </li>
    {/each}
  </ul>
{/if}
