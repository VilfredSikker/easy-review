<script module lang="ts">
  // The theme the button last switched away from. Module scope so it outlives
  // a remount (tab switch, view change) within the session.
  let themeBeforeToggle: string | null = null;
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app } from "$lib/stores/app.svelte";
  import { browser } from "$lib/stores/browser.svelte";
  import { terminal } from "$lib/stores/terminal.svelte";
  import { rightRail } from "$lib/stores/rightRail.svelte";
  import { commandPalette } from "$lib/stores/commandPalette.svelte";
  import { reviewPage, type ReviewPage } from "$lib/stores/reviewPage.svelte";
  import { resolveActivePrNumber } from "$lib/prUrl";
  import { activeDiffSource, sourceToggleState, type DiffSource } from "$lib/sourceToggle";
  import { applyTheme, themeByName } from "$lib/themes";
  import { themeToggleTarget } from "$lib/viewBarModel";
  import { pendingReviewKey, reviewGate } from "$lib/reviewSubmit";
  import { pendingReview } from "$lib/stores/pendingReview.svelte";
  import DisplayMenu from "./DisplayMenu.svelte";
  import ReviewSubmitPopover from "./ReviewSubmitPopover.svelte";
  import { diffSync } from "./diffSync.svelte";

  /** The row under the top bar: which review page is open, where the diff
   *  comes from, how it is drawn, the panel toggles, and Review. */
  const snapshot = $derived(app.snapshot);
  const activeTab = $derived(
    snapshot?.tabs.find((t) => t.is_active) ?? snapshot?.tabs[snapshot?.active_tab ?? 0],
  );

  const page = $derived(reviewPage.current);
  const pages = $derived<{ id: ReviewPage; label: string; key: string; count: number | null }[]>([
    { id: "overview", label: "Overview", key: "1", count: null },
    {
      id: "tour",
      label: "Tour",
      key: "2",
      count: snapshot?.tour?.available ? snapshot.tour.pillars.length : null,
    },
    { id: "diff", label: "Diff", key: "3", count: snapshot ? snapshot.total_count : null },
  ]);

  const prNumber = $derived(resolveActivePrNumber(snapshot));
  const source = $derived(activeDiffSource(snapshot?.mode, snapshot?.tour?.scope));
  const toggle = $derived(
    sourceToggleState({
      prNumber,
      tabKind: activeTab?.kind,
      localBranchCheckedOut: snapshot?.local_branch_checked_out,
    }),
  );

  function selectSource(next: DiffSource) {
    if (next === source) return;
    if (next === "local" && toggle.localAvailable) void app.cmd("set_mode", { mode: "branch" });
    if (next === "pr" && toggle.prAvailable) void app.cmd("set_mode", { mode: "pr_diff", prNumber });
  }

  // Stale is the open diff falling behind origin, never artifact staleness,
  // which findings and the tour report on their own (ADR 0010).
  const diffStale = $derived(snapshot?.diff_stale ?? null);

  // What this button painted, until a snapshot reports a theme change. The
  // patch does not push a snapshot, so without it a second click would read
  // the old theme and switch to the one already showing.
  let paintedTheme = $state<string | null>(null);
  const snapshotTheme = $derived(snapshot?.theme);
  $effect(() => {
    void snapshotTheme;
    paintedTheme = null;
  });
  const themeName = $derived(paintedTheme ?? themeByName(snapshotTheme).name);
  const themeTarget = $derived(themeToggleTarget(themeName, themeBeforeToggle));

  // The theme is the global `display.theme` the TUI also reads (ADR 0005), so
  // it goes through the config patch like a Settings pick. Painting it here
  // keeps the click instant; the next snapshot carries the same name.
  async function toggleTheme(e: MouseEvent) {
    if (e.altKey) {
      app.setMainView("settings");
      return;
    }
    const from = themeName;
    const to = themeTarget;
    try {
      await invoke("apply_config_patch", { patch: { key: "display.theme", value: to } });
    } catch (err) {
      app.showToast("error", `Theme change failed: ${err}`);
      return;
    }
    themeBeforeToggle = from;
    paintedTheme = to;
    const theme = applyTheme(to);
    app.currentSyntaxTheme = theme.syntaxThemeId;
  }

  // The decision is picked in the popover, so a click only opens it; nothing
  // reaches GitHub until Submit review.
  const review = $derived(reviewGate(prNumber, snapshot?.github));
  const reviewKey = $derived(pendingReviewKey(prNumber, snapshot?.github));
  let reviewButton = $state<HTMLButtonElement | null>(null);
  $effect(() => {
    pendingReview.follow(reviewKey);
  });

  const iconBtn =
    "grid h-7 w-7 shrink-0 place-items-center rounded-[7px] hover:bg-card hover:text-fg aria-pressed:bg-card aria-pressed:text-accent aria-pressed:shadow-[inset_0_0_0_1px_var(--color-border)]";
  const segBtn =
    "inline-flex items-center gap-1.5 rounded-[5px] px-[9px] py-0.5 text-[11.5px] font-medium text-fg-3 hover:text-fg aria-pressed:bg-surface aria-pressed:text-fg aria-pressed:shadow-[0_0_0_1px_var(--color-border)] aria-disabled:cursor-not-allowed aria-disabled:opacity-40 aria-disabled:hover:text-fg-3";
</script>

<div class="flex min-w-0 shrink-0 items-center gap-2 border-b border-line bg-bg px-3.5 py-2" data-testid="view-bar">
  <div class="flex gap-0.5 rounded-lg bg-card p-0.5" role="tablist" aria-label="Review views">
    {#each pages as p (p.id)}
      {@const available = reviewPage.available(p.id)}
      <button
        type="button"
        role="tab"
        class="inline-flex items-center gap-1.5 rounded-md px-[11px] py-1 text-[13px] font-medium text-fg-3 hover:text-fg aria-selected:bg-surface aria-selected:text-fg aria-selected:shadow-[0_0_0_1px_var(--color-border)] aria-disabled:cursor-not-allowed aria-disabled:opacity-40 aria-disabled:hover:text-fg-3"
        aria-selected={page === p.id}
        aria-disabled={!available}
        title={available ? `${p.label} (${p.key})` : `${p.label} is not available here`}
        data-testid="view-tab-{p.id}"
        onclick={() => available && reviewPage.show(p.id)}
      >
        {p.label}
        {#if p.count != null}<span class="font-mono text-[11px] text-muted">{p.count}</span>{/if}
        <kbd class="font-mono text-[10px] text-muted max-[880px]:hidden">{p.key}</kbd>
      </button>
    {/each}
  </div>

  <div class="flex-1"></div>

  {#if diffStale}
    <div
      class="inline-flex h-6 shrink-0 items-center gap-1 rounded-md border border-[color-mix(in_srgb,var(--color-warning)_40%,transparent)] bg-[color-mix(in_srgb,var(--color-warning)_14%,transparent)] pl-2 pr-[3px] text-[10.5px] font-semibold uppercase tracking-[0.05em] text-warning"
      title={diffStale.message}
      data-testid="stale-pill"
    >
      <span>Stale</span>
      <button
        type="button"
        class="grid h-5 w-5 place-items-center rounded text-warning hover:bg-[color-mix(in_srgb,var(--color-warning)_20%,transparent)] disabled:cursor-default"
        aria-label="Sync to the new head"
        title={diffStale.message}
        disabled={diffSync.syncing}
        onclick={() => void diffSync.sync()}
      >
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class={diffSync.syncing ? "motion-safe:animate-spin" : ""}
          aria-hidden="true"
        >
          <path d="M3 12a9 9 0 0 1 15-6.7L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-15 6.7L3 16" /><path d="M3 21v-5h5" />
        </svg>
      </button>
    </div>
  {/if}

  {#if activeTab}
    <div class="inline-flex shrink-0 rounded-[7px] bg-card p-0.5 max-[880px]:hidden" role="group" aria-label="Diff source">
      <button
        type="button"
        class={segBtn}
        aria-pressed={source === "local"}
        aria-disabled={!toggle.localAvailable}
        title={toggle.localReason ?? "The branch as checked out, with local changes"}
        onclick={() => selectSource("local")}
      >
        Local Branch
      </button>
      <button
        type="button"
        class={segBtn}
        aria-pressed={source === "pr"}
        aria-disabled={!toggle.prAvailable}
        title={toggle.prReason ?? "The pull request's head against its base"}
        onclick={() => selectSource("pr")}
      >
        PR Diff
        {#if prNumber != null}<span class="font-mono text-[10.5px] text-muted">#{prNumber}</span>{/if}
      </button>
    </div>
  {/if}

  {#if page !== "overview"}
    <DisplayMenu />
  {/if}

  <span class="h-[18px] w-px shrink-0 bg-border max-[880px]:hidden" aria-hidden="true"></span>

  <button
    type="button"
    class="{iconBtn} text-fg-3"
    aria-label="Search and commands"
    aria-pressed={commandPalette.open}
    title="Search and commands (⌘K)"
    onclick={() => commandPalette.toggle()}
  >
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true">
      <circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" />
    </svg>
  </button>
  <button
    type="button"
    class="{iconBtn} text-fg-3"
    aria-label="Toggle findings panel"
    aria-pressed={!rightRail.collapsed}
    title="Findings & risks panel (])"
    onclick={() => rightRail.toggle()}
  >
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
      <rect x="1.75" y="2.5" width="12.5" height="11" rx="2" /><path d="M10 2.5v11" />
    </svg>
  </button>
  <button
    type="button"
    class="{iconBtn} text-fg-3"
    aria-label="Toggle terminal"
    aria-pressed={terminal.open}
    title="Terminal (⌃`)"
    onclick={() => terminal.toggle()}
  >
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
      <rect x="1.75" y="2.5" width="12.5" height="11" rx="2" /><path d="M4.5 6.5l2 1.5-2 1.5M8 10h3.5" />
    </svg>
  </button>
  <button
    type="button"
    class="{iconBtn} text-fg-3"
    aria-label="Toggle browser"
    aria-pressed={browser.layout === "split"}
    title="Browser split (⌘B)"
    onclick={() => void browser.setLayout(browser.layout === "split" ? "hidden" : "split")}
  >
    <svg width="15" height="15" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
      <rect x="1.75" y="2.5" width="12.5" height="11" rx="2" /><path d="M1.75 5.5h12.5M10 5.5v8" />
    </svg>
  </button>

  <button
    type="button"
    class="inline-flex h-7 shrink-0 items-center rounded-[7px] border border-border bg-surface px-2.5 text-fg hover:border-[color-mix(in_srgb,var(--color-fg)_25%,transparent)]"
    aria-label="Switch to the {themeTarget} theme"
    title="Switch to {themeTarget} · ⌥-click for all themes"
    data-testid="theme-button"
    onclick={toggleTheme}
  >
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
      <circle cx="8" cy="8" r="5.5" /><path d="M8 2.5v11" /><path d="M8 2.5a5.5 5.5 0 010 11z" fill="currentColor" />
    </svg>
  </button>

  <button
    type="button"
    bind:this={reviewButton}
    class="inline-flex h-7 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-[7px] bg-accent px-2.5 font-medium text-on-accent hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
    disabled={!review.enabled}
    aria-haspopup="dialog"
    aria-expanded={pendingReview.open}
    title={review.reason ??
      (pendingReview.count
        ? `${pendingReview.count} pending comment${pendingReview.count === 1 ? "" : "s"} · submit your review`
        : "Comment, approve or request changes")}
    data-testid="review-button"
    onclick={() => pendingReview.toggle()}
  >
    Review
    {#if pendingReview.count > 0}
      <span
        class="inline-grid h-[17px] min-w-[17px] place-items-center rounded-[9px] bg-on-accent px-1 font-mono text-[10.5px] text-accent"
        data-testid="review-count">{pendingReview.count}</span
      >
    {/if}
    <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <path d="M4 6l4 4 4-4" />
    </svg>
  </button>
  {#if review.enabled && prNumber != null}
    <ReviewSubmitPopover {prNumber} anchor={reviewButton} />
  {/if}
</div>
