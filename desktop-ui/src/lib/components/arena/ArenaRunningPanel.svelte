<script lang="ts">
  import type { ArenaStartConfig } from "$lib/stores/arena.svelte";
  import type { ArenaRunSnapshot } from "$lib/types/arena";

  interface Props {
    open: boolean;
    minimized: boolean;
    config: ArenaStartConfig | null;
    snapshot: ArenaRunSnapshot | null;
    onMinimize: () => void;
    onRestore: () => void;
    onCancel: () => void;
    onComplete: () => void;
  }

  const {
    open,
    minimized,
    config,
    snapshot,
    onMinimize,
    onRestore,
    onCancel,
    onComplete,
  }: Props = $props();

  const isArena = $derived((config?.reviewers.length ?? 0) >= 2);
  const totalRounds = $derived(isArena ? (config?.rounds ?? 3) : 1);

  const progress = $derived.by(() => {
    const st = snapshot?.run.status;
    if (!st) return { round: 1, label: "Starting…" };
    if (st === "queued") return { round: 0, label: "Queued" };
    if (st === "complete") return { round: totalRounds, label: "Complete" };
    if (st === "cancelled") return { round: 0, label: "Cancelled" };
    if (st === "failed") return { round: 0, label: "Failed" };
    if (typeof st === "object" && "running" in st) {
      return { round: st.running.round, label: `Round ${st.running.round}` };
    }
    return { round: 1, label: "Running" };
  });

  const pct = $derived(
    totalRounds > 0 ? Math.min(100, Math.round((progress.round / totalRounds) * 100)) : 0,
  );

  $effect(() => {
    if (snapshot?.run.status === "complete") {
      onComplete();
    }
  });
</script>

{#if open && config}
{#if minimized}
  <button
    type="button"
    onclick={onRestore}
    class="fixed bottom-20 right-6 z-[190] flex items-center gap-2 rounded-full border border-[var(--arena-border)] bg-[var(--arena-bg-2)] px-4 py-2 shadow-lg"
  >
    <span class="text-[var(--arena-periwinkle)]">●</span>
    <span class="text-[11px] font-medium text-[var(--arena-fg)]">
      {isArena ? `Arena · ${progress.label}` : "Review running"}
    </span>
    <span class="mono text-[10px] text-[var(--arena-fg-subtle)]">{pct}%</span>
  </button>
{:else}
  <div
    class="fixed inset-0 z-[190] flex items-center justify-center bg-black/50 p-6"
    role="dialog"
    aria-label="Review in progress"
  >
    <div
      class="w-full max-w-md rounded-xl border border-[var(--arena-border)] bg-[var(--arena-bg-1)] p-6 shadow-2xl"
    >
      <h2 class="text-[14px] font-semibold text-[var(--arena-fg)]">
        {isArena
          ? `Arena in progress · Round ${progress.round} of ${totalRounds}`
          : "Review in progress"}
      </h2>
      <p class="mt-1 text-[11px] text-[var(--arena-fg-subtle)]">{progress.label}</p>
      <div class="mt-4 h-2 overflow-hidden rounded-full bg-[var(--arena-bg-0)]">
        <div
          class="h-full bg-[var(--arena-periwinkle)] transition-all duration-300"
          style="width: {pct}%"
        ></div>
      </div>
      <div class="mt-6 flex justify-end gap-2">
        <button
          type="button"
          class="rounded-md border border-[var(--arena-border)] px-3 py-1.5 text-[11px] text-[var(--arena-fg-muted)]"
          onclick={onMinimize}
        >Minimize</button>
        <button
          type="button"
          class="rounded-md border border-[var(--arena-err)] px-3 py-1.5 text-[11px] text-[var(--arena-err)]"
          onclick={onCancel}
        >Cancel</button>
      </div>
    </div>
  </div>
{/if}
{/if}
