<script lang="ts">
  import type { AiSnapshot, PrSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { resolveActivePrUrl, githubStatusForActiveTab, resolveActivePrNumber } from "$lib/prUrl";
  import { resolveContextIdentity } from "$lib/contextIdentity";
  import { panelThreads } from "./rpanel/commentsPanel";
  import BranchCard from "./BranchCard.svelte";
  import ChecklistCard from "./ChecklistCard.svelte";
  import TriageCard from "./TriageCard.svelte";
  import CommentsCard from "./CommentsCard.svelte";
  import AgentOutputCard from "./AgentOutputCard.svelte";
  import DiagramsCard from "./DiagramsCard.svelte";
  import FindingsTab from "./rpanel/FindingsTab.svelte";
  import RisksTab from "./rpanel/RisksTab.svelte";
  import CommentsTab from "./rpanel/CommentsTab.svelte";
  import { useCommentAutoPull } from "./rpanel/commentAutoPull.svelte";
  import {
    PRIMARY_TABS,
    SECONDARY_TABS,
    SECONDARY_TAB_LABELS,
    isSecondaryTab,
    type PrimaryRightPanelTab,
  } from "./rpanel/rightPanelTabs";
  import { rightPanelTab } from "$lib/stores/rightPanelTab.svelte";

  interface Props {
    ai: AiSnapshot | null;
    pr: PrSnapshot | null;
    width?: number;
    dragging?: boolean;
    onResizeStart?: (e: MouseEvent) => void;
    onCollapseToggle?: () => void;
  }

  const {
    ai,
    pr,
    width = 300,
    dragging = false,
    onResizeStart,
    onCollapseToggle,
  }: Props = $props();

  const activeTab = $derived(rightPanelTab.active);

  useCommentAutoPull();

  const currentFile = $derived(app.snapshot?.files[app.snapshot.selected_file]?.path ?? null);

  // Tab counts are of what each tab lists: open findings, changed files, and
  // the threads the Comments tab shows (GitHub ones under the diff's filters).
  const files = $derived(app.snapshot?.files);
  const findingCount = $derived(ai?.findings.length ?? 0);
  const fileCount = $derived(files?.length ?? 0);
  const threadCount = $derived(panelThreads(ai?.threads, app.commentVisibility, files).length);
  const TAB_LABEL: Record<PrimaryRightPanelTab, string> = {
    findings: "Findings",
    risks: "Risks",
    comments: "Comments",
  };
  const tabCount = $derived<Record<PrimaryRightPanelTab, number>>({
    findings: findingCount,
    risks: fileCount,
    comments: threadCount,
  });

  let moreOpen = $state(false);
  let moreEl = $state<HTMLDivElement | null>(null);

  function closeMoreOnOutside(e: MouseEvent) {
    if (moreOpen && moreEl && !moreEl.contains(e.target as Node)) moreOpen = false;
  }

  // ── Branch & PR (until the Overview aside carries it) ──────────────────────
  const identity = $derived(resolveContextIdentity(app.snapshot));
  const currentWorktree = $derived(app.snapshot?.worktrees.find((w) => w.is_current) ?? null);
  const activeAppTab = $derived(app.snapshot?.tabs?.find((t) => t.is_active) ?? null);
  const displayPrNumber = $derived(resolveActivePrNumber(app.snapshot));
  const displayPrUrl = $derived(resolveActivePrUrl(app.snapshot));
  const githubForTab = $derived(githubStatusForActiveTab(app.snapshot));
  const tabOwnsPr = $derived(activeAppTab?.pr_number != null);
  const isMerged = $derived(
    githubForTab?.state === "MERGED" || (!tabOwnsPr && (currentWorktree?.is_merged ?? false)),
  );
  const checksStatus = $derived.by((): "success" | "pending" | "failure" | null => {
    const checks = githubForTab?.checks;
    if (!checks || checks.length === 0) return null;
    if (checks.some((c) => c.conclusion === "FAILURE" || c.conclusion === "fail")) return "failure";
    if (checks.some((c) => c.status === "PENDING")) return "pending";
    return "success";
  });
  // Whole-diff counts from the engine, so a file filter never shrinks them.
  const totalAdds = $derived(app.snapshot?.diff_stats?.total.additions ?? 0);
  const totalDels = $derived(app.snapshot?.diff_stats?.total.deletions ?? 0);
</script>

<svelte:window
  onclick={closeMoreOnOutside}
  onkeydown={(e) => {
    if (moreOpen && e.key === "Escape") moreOpen = false;
  }}
/>

<aside
  class="shrink-0 bg-surface border-l border-hairline overflow-hidden flex flex-col relative
    max-[880px]:absolute max-[880px]:right-0 max-[880px]:inset-y-0 max-[880px]:z-20 max-[880px]:max-w-[92%] max-[880px]:shadow-2xl"
  style="width: {width}px"
  aria-label="Findings, risks and comments"
>
  {#if onResizeStart}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions (mouse-only drag handle; keyboard resizing is not implemented) -->
    <div
      class="absolute -left-[2px] top-0 bottom-0 w-1 cursor-ew-resize z-10 hover:bg-accent/40 {dragging ? 'bg-accent/60' : ''}"
      onmousedown={onResizeStart}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize right panel"
    ></div>
  {/if}

  <div class="flex items-center gap-0.5 pt-2 pr-2 pl-2.5 border-b border-hairline shrink-0" role="tablist">
    {#each PRIMARY_TABS as tab (tab)}
      {@const selected = activeTab === tab}
      <button
        type="button"
        role="tab"
        aria-selected={selected}
        onclick={() => rightPanelTab.set(tab)}
        class="px-2 pt-1.5 pb-2 -mb-px text-[12.5px] font-medium border-b-2 transition-colors
          {selected ? 'text-fg border-accent' : 'text-fg-2 border-transparent hover:text-fg'}"
      >{TAB_LABEL[tab]} <span class="text-fg-3 font-normal">{tabCount[tab]}</span></button>
    {/each}
    {#if isSecondaryTab(activeTab)}
      <button
        type="button"
        role="tab"
        aria-selected="true"
        class="px-2 pt-1.5 pb-2 -mb-px text-[12.5px] font-medium border-b-2 text-fg border-accent truncate"
      >{SECONDARY_TAB_LABELS[activeTab]}</button>
    {/if}
    <span class="flex-1"></span>
    <div class="relative self-center" bind:this={moreEl}>
      <button
        type="button"
        onclick={() => (moreOpen = !moreOpen)}
        aria-haspopup="menu"
        aria-expanded={moreOpen}
        title="Branch & PR, triage and diagrams"
        aria-label="More panels"
        class="w-6 h-6 rounded-md flex items-center justify-center text-fg-3 hover:bg-hover hover:text-fg"
      >⋯</button>
      {#if moreOpen}
        <div
          role="menu"
          class="absolute right-0 top-7 z-30 min-w-40 py-1 rounded-lg border border-border bg-card shadow-xl"
        >
          {#each SECONDARY_TABS as tab (tab)}
            <button
              type="button"
              role="menuitem"
              onclick={() => {
                rightPanelTab.set(tab);
                moreOpen = false;
              }}
              class="w-full text-left px-3 py-1.5 text-[12px] hover:bg-hover {activeTab === tab ? 'text-fg' : 'text-fg-2'}"
            >{SECONDARY_TAB_LABELS[tab]}</button>
          {/each}
        </div>
      {/if}
    </div>
    {#if onCollapseToggle}
      <button
        type="button"
        onclick={onCollapseToggle}
        title="Close panel (])"
        aria-label="Close panel"
        class="w-6 h-6 rounded-md flex items-center justify-center text-fg-3 hover:bg-hover hover:text-fg text-[15px] leading-none self-center"
      >×</button>
    {/if}
  </div>

  <div class="flex-1 overflow-auto p-3 grid gap-3 content-start" role="tabpanel">
    {#if activeTab === "findings"}
      <FindingsTab {ai} {currentFile} />
    {:else if activeTab === "risks"}
      <RisksTab {ai} {currentFile} />
    {:else if activeTab === "comments"}
      <CommentsTab {ai} {currentFile} />
    {:else if activeTab === "branch"}
      {#if app.snapshot}
        <BranchCard
          branch={identity.branch}
          base={identity.base}
          {pr}
          reviewed_count={app.snapshot.reviewed_count}
          total_count={app.snapshot.total_count}
          additions={totalAdds}
          deletions={totalDels}
          checks_status={checksStatus}
          pr_number={displayPrNumber}
          is_merged={isMerged}
          github_url={displayPrUrl}
          github={githubForTab}
        />
      {/if}
      {#if ai}
        <!-- The panel already auto-pulls; this card keeps the push and
             submit-review controls. -->
        <CommentsCard {ai} active={false} />
      {/if}
    {:else if activeTab === "triage"}
      {#if ai?.triage}
        <TriageCard triage={ai.triage} />
      {/if}
      <!-- The TUI shows the checklist whenever the AI panel is up, with a
           fallback when the bucket has none — same gate here. -->
      {#if ai && (ai.checklist || ai.has_review_json)}
        <ChecklistCard checklist={ai.checklist} />
      {/if}
      <AgentOutputCard />
    {:else if activeTab === "context"}
      {#if ai}
        <DiagramsCard {ai} />
      {/if}
    {/if}
  </div>
</aside>
