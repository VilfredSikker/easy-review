<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { layoutPanels } from "$lib/stores/layoutPanels.svelte";
  import { resolveContextIdentity, resolveContextTitle } from "$lib/contextIdentity";
  import { deriveLiveIndicator } from "$lib/liveIndicator";
  import { nextCodeFilter } from "$lib/codeFilter";
  import { reviewRing } from "$lib/runningPill";
  import RunningPill from "./RunningPill.svelte";

  /** Review window top bar: breadcrumb with base and line counters, Live, the
   *  running-AI pill and the reviewed ring. */
  const snapshot = $derived(app.snapshot);
  const activeTab = $derived(
    snapshot?.tabs.find((t) => t.is_active) ?? snapshot?.tabs[snapshot?.active_tab ?? 0],
  );
  const crumb = $derived(resolveContextTitle(snapshot));
  const identity = $derived(resolveContextIdentity(snapshot));

  // Whole-diff counts from the engine, so a file filter never shrinks them.
  const totalStat = $derived(snapshot?.diff_stats?.total ?? { additions: 0, deletions: 0 });
  const codeStat = $derived(snapshot?.diff_stats?.code ?? totalStat);
  /** Only worth a second pair when tests/generated/docs actually moved the number. */
  const showCode = $derived(
    codeStat.additions !== totalStat.additions || codeStat.deletions !== totalStat.deletions,
  );
  const codeFilterActive = $derived(nextCodeFilter(snapshot?.filter) === null);

  function toggleCodeFilter() {
    const next = nextCodeFilter(snapshot?.filter);
    if (next) void app.cmd("set_filter", { query: next });
    else void app.cmd("clear_filter");
  }

  const mode = $derived(snapshot?.mode ?? "branch");
  const isPrView = $derived(
    activeTab?.kind === "remote_pr" || mode === "pr" || (mode === "tour" && snapshot?.tour?.scope === "pr"),
  );
  const live = $derived(
    deriveLiveIndicator({ isPrView, diffStale: snapshot?.diff_stale, branch: identity.branch ?? "" }),
  );
  const liveDot = {
    live: "bg-add-fg shadow-[0_0_0_3px_color-mix(in_srgb,var(--color-add-fg)_22%,transparent)] motion-safe:animate-pulse",
    stale: "bg-warning shadow-[0_0_0_3px_color-mix(in_srgb,var(--color-warning)_22%,transparent)]",
    local: "bg-action shadow-[0_0_0_3px_color-mix(in_srgb,var(--color-action)_22%,transparent)]",
  } as const;

  const ring = $derived(reviewRing(snapshot?.reviewed_count ?? 0, snapshot?.total_count ?? 0));

  /** Remote PR diffs come from GitHub, so only local views can re-base. */
  const canChangeBase = $derived(activeTab?.kind !== "remote_pr");
  let editingBase = $state(false);
  let baseDraft = $state("");

  function startEditBase() {
    baseDraft = identity.base ?? "";
    editingBase = true;
  }

  async function commitBase() {
    const draft = baseDraft.trim();
    editingBase = false;
    if (!draft || draft === identity.base) return;
    try {
      await app.cmd("change_base", { branch: draft });
      app.showToast("success", `Now diffing against ${draft}`);
    } catch (e) {
      app.showToast("error", `Change base failed: ${e}`);
    }
  }

  function handleBaseKey(e: KeyboardEvent) {
    if (e.key === "Enter") void commitBase();
    else if (e.key === "Escape") editingBase = false;
  }
</script>

<header
  class="flex h-[42px] min-w-0 shrink-0 items-center gap-2.5 border-b border-line px-3.5"
  data-testid="top-bar"
>
  <button
    type="button"
    class="grid h-7 w-7 shrink-0 place-items-center rounded-md hover:bg-hover {layoutPanels.left ? 'text-fg' : 'text-fg-3'}"
    aria-label="Toggle side menu"
    aria-pressed={layoutPanels.left}
    title="Toggle side menu"
    onclick={() => app.togglePanel("left")}
  >
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
      <rect x="1.75" y="2.5" width="12.5" height="11" rx="2" /><path d="M6 2.5v11" />
    </svg>
  </button>

  <nav class="flex min-w-0 flex-1 items-center gap-2 whitespace-nowrap" aria-label="Breadcrumb">
    {#if crumb.repo}
      <span class="text-fg-3 max-[520px]:hidden" data-testid="crumb-repo">{crumb.repo}</span>
      <span class="text-muted max-[520px]:hidden" aria-hidden="true">/</span>
    {/if}
    {#if crumb.prNumber != null}
      <span class="font-mono text-[12px] text-fg-3 max-[520px]:hidden">#{crumb.prNumber}</span>
    {/if}
    <span class="min-w-0 truncate font-medium text-fg" data-testid="crumb-title">{crumb.title}</span>

    <span class="inline-flex shrink-0 items-center gap-1.5 text-[11.5px] max-[880px]:hidden" data-testid="crumb-stat">
      {#if identity.base}
        <span class="text-muted">base</span>
        {#if editingBase}
          <!-- svelte-ignore a11y_autofocus (mounts only after the user clicks the base chip to edit it) -->
          <input
            class="w-36 rounded border border-border bg-bg px-1.5 py-px font-mono text-[11px] text-fg outline-none"
            bind:value={baseDraft}
            onkeydown={handleBaseKey}
            onblur={commitBase}
            placeholder="branch or origin/branch"
            aria-label="Compare base"
            data-testid="context-base-input"
            autofocus
          />
        {:else if canChangeBase}
          <button
            type="button"
            class="rounded bg-hover px-[5px] font-mono text-[11px] text-fg-3 hover:text-fg"
            title="Change compare base — click to edit (e.g. a release branch)"
            data-testid="context-base"
            onclick={startEditBase}
          >
            {identity.base}
          </button>
        {:else}
          <span class="rounded bg-hover px-[5px] font-mono text-[11px] text-fg-3" data-testid="context-base">{identity.base}</span>
        {/if}
      {/if}
      {#if totalStat.additions > 0 || totalStat.deletions > 0}
        <span class="font-mono"><span class="text-add-fg">+{totalStat.additions}</span> <span class="text-del-fg">−{totalStat.deletions}</span></span>
        {#if showCode}
          <span class="text-muted" aria-hidden="true">·</span>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded px-1 font-mono hover:bg-hover {codeFilterActive ? 'bg-hover ring-1 ring-hairline' : ''}"
            aria-pressed={codeFilterActive}
            title={codeFilterActive
              ? "Showing code files only — click to clear the filter"
              : "Production lines only: no tests, generated files or docs — click to show only these files"}
            data-testid="context-code-stat"
            onclick={toggleCodeFilter}
          >
            <span class="font-sans text-muted">code</span>
            <span><span class="text-add-fg">+{codeStat.additions}</span> <span class="text-del-fg">−{codeStat.deletions}</span></span>
          </button>
        {/if}
      {/if}
    </span>
  </nav>

  <span
    class="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[12px] text-fg-3"
    title={live.state === "stale" ? live.label : "er re-reads the diff when the branch moves"}
    data-testid="live-indicator"
    data-state={live.state}
  >
    <i class="h-1.5 w-1.5 rounded-full {liveDot[live.state]}" aria-hidden="true"></i>
    <span class="max-w-[260px] truncate max-[880px]:hidden">{live.label}</span>
  </span>

  <RunningPill />

  <span
    class="inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[12px] text-fg-3"
    title={ring.label}
    data-testid="review-ring"
  >
    <svg width="16" height="16" viewBox="0 0 16 16" class="-rotate-90" aria-hidden="true">
      <circle cx="8" cy="8" r="6" fill="none" stroke="var(--color-ink-600)" stroke-width="2" />
      <circle
        cx="8"
        cy="8"
        r="6"
        fill="none"
        stroke="var(--color-add-fg)"
        stroke-width="2"
        stroke-linecap="round"
        stroke-dasharray="{ring.arc} {ring.circumference}"
      />
    </svg>
    <span class="max-[520px]:hidden">{ring.label}</span>
  </span>
</header>
