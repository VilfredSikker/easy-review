<script module lang="ts">
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- module-level once-per-PR cache, never read reactively; autoPulledFor carries the state
  const autoPulledKeys = new Set<string>();
</script>

<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import {
    commentThreadsFromDiff,
    hiddenCommentsHint,
    toggleCommentFilter,
    visibleCommentThreads as filterVisibleComments,
  } from "$lib/commentVisibility";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import InlineThread from "$lib/components/InlineThread.svelte";
  import { commentAutoPullKey } from "$lib/prUrl";

  interface Props {
    ai: AiSnapshot;
    /** When false, skip auto-pull (e.g. Branch tab not active). */
    active?: boolean;
  }

  const { ai, active = true }: Props = $props();

  let manualRefreshing = $state(false);
  let autoPulledFor = $state<string | null>(null);

  const files = $derived(app.snapshot?.files);
  const commentThreads = $derived(commentThreadsFromDiff(ai.threads, files));
  const vis = $derived(app.commentVisibility);
  const visibleCommentThreads = $derived(filterVisibleComments(ai.threads, vis, files));
  const annotationCount = $derived(app.snapshot?.ui_annotations?.length ?? 0);
  const refreshing = $derived(
    manualRefreshing || (app.snapshot?.bg_loading?.gh_comments ?? false),
  );

  function currentAutoPullKey(): string | null {
    return commentAutoPullKey(app.snapshot);
  }

  function rememberAutoPull(key: string) {
    autoPulledKeys.add(key);
    if (autoPulledKeys.size > 128) {
      const oldest = autoPulledKeys.values().next().value;
      if (oldest) autoPulledKeys.delete(oldest);
    }
  }

  $effect(() => {
    if (!active) return;
    if (app.pendingTabSwitch) return;
    const key = currentAutoPullKey();
    if (!key) return;
    if (autoPulledFor === key || autoPulledKeys.has(key)) return;
    autoPulledFor = key;
    void (async () => {
      await app.cmd("pull_github_comments");
      if (app.pendingTabSwitch || currentAutoPullKey() !== key) {
        if (autoPulledFor === key) autoPulledFor = null;
        return;
      }
      rememberAutoPull(key);
    })();
  });

  async function onRefresh() {
    manualRefreshing = true;
    try {
      await app.cmd("pull_github_comments", { force: true });
    } finally {
      manualRefreshing = false;
    }
  }

  function scrollToAnnotations() {
    const el = document.getElementById("ui-annotations-card");
    el?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

</script>

<Card>
  <div class="flex items-center justify-between mb-3 gap-2">
    <SectionLabel>Comments</SectionLabel>
    <div class="flex items-center gap-2 min-w-0">
      <span class="flex items-center gap-1 text-[10px] mono text-comment shrink-0"><svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>{ai.local_comment_count} local</span>
      {#if ai.github_comment_count > 0}
        <span class="flex items-center gap-1 text-[10px] mono text-muted shrink-0">{ai.github_comment_count} GitHub</span>
      {/if}
      {#if annotationCount > 0}
        <button
          type="button"
          onclick={scrollToAnnotations}
          class="text-[10px] mono text-muted hover:text-fg-2 px-1 py-0.5 rounded border border-hairline shrink-0"
          title="Jump to UI annotations"
        >+ {annotationCount} annotation{annotationCount === 1 ? "" : "s"}</button>
      {/if}
      <button
        type="button"
        class={`p-1 rounded transition-colors shrink-0 ${refreshing ? "text-muted opacity-50 cursor-not-allowed" : "text-muted hover:text-fg-2 hover:bg-fg-3/10"}`}
        onclick={onRefresh}
        disabled={refreshing}
        title="Refresh comments from GitHub"
        aria-label="Refresh comments from GitHub"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
          class:animate-spin={refreshing}>
          <path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>
        </svg>
      </button>
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-1.5 mb-3">
    <button
      type="button"
      onclick={() => app.setCommentVisibility(toggleCommentFilter(vis, "hideOutdated"))}
      class="px-2 py-1 rounded text-[10px] border {vis.hideOutdated ? 'bg-hover border-border text-fg' : 'border-hairline text-muted hover:text-fg-2'}"
      title="Hide outdated GitHub comments in the side panel and inline diff"
      aria-pressed={vis.hideOutdated}
    >Hide outdated</button>
    <button
      type="button"
      onclick={() => app.setCommentVisibility(toggleCommentFilter(vis, "hideResolved"))}
      class="px-2 py-1 rounded text-[10px] border {vis.hideResolved ? 'bg-hover border-border text-fg' : 'border-hairline text-muted hover:text-fg-2'}"
      title="Hide resolved GitHub comments in the side panel and inline diff"
      aria-pressed={vis.hideResolved}
    >Hide resolved</button>
    <button
      type="button"
      onclick={() => app.setCommentVisibility(toggleCommentFilter(vis, "hideAll"))}
      class="px-2 py-1 rounded text-[10px] border {vis.hideAll ? 'bg-del-bg border-del-fg/30 text-del-fg' : 'border-hairline text-muted hover:text-fg-2'}"
      title="Hide every GitHub comment in the side panel and inline diff"
      aria-pressed={vis.hideAll}
    >Hide all</button>
    {#if visibleCommentThreads.length !== commentThreads.length}
      <span class="text-[10px] text-muted mono">{visibleCommentThreads.length}/{commentThreads.length} shown</span>
    {/if}
  </div>

  <div class="space-y-2">
    {#each visibleCommentThreads as thread (thread.id)}
      <InlineThread {thread} hunk_idx={0} variant="panel" />
    {:else}
      <p class="text-[11px] text-muted py-1">
        {#if refreshing}
          Fetching comments from GitHub…
        {:else if commentThreads.length > 0}
          {hiddenCommentsHint(commentThreads.length, vis)}
        {:else}
          No comments on this diff
        {/if}
      </p>
    {/each}
  </div>

  <button type="button"
    onclick={() => app.setMainView("export-review")}
    class="mt-3 w-full px-3 py-1.5 text-xs rounded-md border border-border hover:bg-hover text-fg-2 flex items-center justify-center gap-2"
  >
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
    Export to coding agent
  </button>
</Card>
