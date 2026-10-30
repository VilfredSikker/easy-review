<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { app } from "$lib/stores/app.svelte";
  import { aggregateJobs } from "$lib/runningJobs";
  import { otherJobs, runningPillView } from "$lib/runningPill";
  import JobsPopover from "./JobsPopover.svelte";

  /** The top bar's AI pill: hidden while nothing runs, opens the jobs popover. */
  const snapshot = $derived(app.snapshot);

  let now = $state(Date.now());
  $effect(() => {
    const handle = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(handle);
  });

  // Agent commands carry no timestamps; remember when each was first seen
  // finished so it stays in the done window instead of vanishing at once.
  const commandDoneAt = new SvelteMap<string, number>();
  $effect(() => {
    const commands = snapshot?.agent_commands ?? [];
    untrack(() => {
      for (const c of commands) {
        const finished = c.status === "done" || c.status === "failed";
        if (!finished) commandDoneAt.delete(c.name);
        else if (!commandDoneAt.has(c.name)) commandDoneAt.set(c.name, Date.now());
      }
    });
  });

  const flow = $derived(snapshot?.review_flow ?? null);
  const jobs = $derived(
    aggregateJobs({
      backgroundTasks: snapshot?.background_tasks,
      agentCommands: snapshot?.agent_commands,
      arenaRuns: snapshot?.background_arena_runs,
      tabs: snapshot?.tabs ?? [],
      activeTabIdx: snapshot?.active_tab ?? null,
      nowMs: now,
      commandDoneAtMs: commandDoneAt,
    }),
  );
  const view = $derived(runningPillView(flow, jobs));

  let open = $state(false);
  let pillEl = $state<HTMLButtonElement | null>(null);
  let anchor = $state<{ top: number; right: number } | null>(null);

  function toggle() {
    if (open) {
      open = false;
      return;
    }
    const r = pillEl?.getBoundingClientRect();
    // A pill near the bottom (the floating fallback) opens the popover from
    // the top instead of running it off-screen.
    anchor = r && r.top < window.innerHeight / 2 ? { top: r.bottom + 6, right: r.right } : { top: 48, right: r?.right ?? window.innerWidth - 8 };
    open = true;
  }

  const toneClass = {
    running: "border-accent/45 bg-accent/10",
    need: "border-warning/55 bg-warning/12",
    done: "border-add-fg/45 bg-add-fg/10",
    failed: "border-del-fg/45 bg-del-fg/10",
  } as const;
</script>

{#if view}
  <button
    bind:this={pillEl}
    type="button"
    class="inline-flex h-6 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full border pl-1.5 pr-[9px] text-[12px] font-medium text-fg {toneClass[view.tone]}"
    aria-haspopup="dialog"
    aria-expanded={open}
    title="AI in progress"
    data-testid="running-pill"
    data-tone={view.tone}
    onclick={toggle}
  >
    {#if view.tone === "running"}
      <!-- Indeterminate: the backend reports no per-job percentage. -->
      <svg width="14" height="14" viewBox="0 0 16 16" class="motion-safe:animate-spin" aria-hidden="true">
        <circle cx="8" cy="8" r="6" fill="none" stroke="var(--color-ink-600)" stroke-width="2" />
        <circle cx="8" cy="8" r="6" fill="none" stroke="var(--color-accent)" stroke-width="2" stroke-linecap="round" stroke-dasharray="10 40" />
      </svg>
    {:else if view.tone === "need"}
      <span class="text-warning" aria-hidden="true">●</span>
    {:else if view.tone === "done"}
      <span class="text-add-fg" aria-hidden="true">✓</span>
    {:else}
      <span class="text-del-fg" aria-hidden="true">✕</span>
    {/if}
    <span>{view.text}</span>
    {#if view.extra > 0}<span class="text-muted">+{view.extra}</span>{/if}
  </button>
{/if}

<JobsPopover
  open={open && (view != null || flow != null)}
  onClose={() => (open = false)}
  {anchor}
  {flow}
  jobs={otherJobs(jobs, flow)}
  tasks={snapshot?.background_tasks ?? []}
  erDir={snapshot?.review_bucket_er_dir ?? null}
  mode={snapshot?.mode}
  nowMs={now}
/>
