<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { panelThreads } from "./rpanel/commentsPanel";
  import { threadsByAuthor } from "./rpanel/commentsAuthor";
  import { commentsAuthorFilter } from "./rpanel/commentsAuthorFilter.svelte";
  import { reviewPageTabKey } from "$lib/stores/reviewPageModel";
  import { scopeToReviewer } from "./rpanel/findingsPanel";
  import { useReviewerFilterSync } from "./rpanel/reviewerFilter.svelte";
  import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";
  import FindingsTab from "./rpanel/FindingsTab.svelte";
  import RisksTab from "./rpanel/RisksTab.svelte";
  import CommentsTab from "./rpanel/CommentsTab.svelte";
  import { useCommentAutoPull } from "./rpanel/commentAutoPull.svelte";
  import { PRIMARY_TABS, isSecondaryTab, type PrimaryRightPanelTab } from "./rpanel/rightPanelTabs";
  import { rightPanelTab } from "$lib/stores/rightPanelTab.svelte";

  interface Props {
    ai: AiSnapshot | null;
    width?: number;
    dragging?: boolean;
    onResizeStart?: (e: MouseEvent) => void;
    onCollapseToggle?: () => void;
  }

  const {
    ai,
    width = 300,
    dragging = false,
    onResizeStart,
    onCollapseToggle,
  }: Props = $props();

  // Branch & PR, Triage and Diagrams live on the Overview now; a tab name left
  // in storage from before shows Findings rather than an empty panel.
  const activeTab = $derived<PrimaryRightPanelTab>(
    isSecondaryTab(rightPanelTab.active) ? "findings" : rightPanelTab.active,
  );

  useCommentAutoPull();
  useReviewerFilterSync(() => ai);

  const currentFile = $derived(app.snapshot?.files[app.snapshot.selected_file]?.path ?? null);

  // Tab counts are of what each tab lists: open findings (under the reviewer
  // filter the diff applies), changed files, and
  // the threads the Comments tab shows (GitHub ones under the diff's filters).
  const files = $derived(app.snapshot?.files);
  const findingCount = $derived(scopeToReviewer(ai, aiReviewFilter.filter).open.length);
  const fileCount = $derived(files?.length ?? 0);
  const threadCount = $derived(
    threadsByAuthor(
      panelThreads(ai?.threads, app.commentVisibility, files),
      commentsAuthorFilter.authorFor(reviewPageTabKey(app.snapshot)),
    ).length,
  );
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
</script>

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
    <span class="flex-1"></span>
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
    {/if}
  </div>
</aside>
