<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import { openExportModal } from "$lib/components/ExportModal.svelte";
  import InlineThread from "$lib/components/InlineThread.svelte";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  let pushMode = $state<null | "review" | "individual">(null);
  let decision = $state<"comment" | "approve" | "changes">("comment");
  let summary = $state("");
  let submitting = $state(false);

  const commentThreads = $derived(ai.threads.filter((t) => t.kind === "comment"));
  const visibleCommentThreads = $derived.by(() => {
    const visibility = app.commentVisibility;
    if (visibility.hideAll) return [];
    return commentThreads.filter(
      (thread) =>
        !(visibility.hideResolved && thread.resolved) &&
        !(visibility.hideOutdated && thread.stale),
    );
  });
  const annotationCount = $derived(app.snapshot?.ui_annotations?.length ?? 0);

  function scrollToAnnotations() {
    const el = document.getElementById("ui-annotations-card");
    el?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  }

  const ghEvent = $derived(
    decision === "approve" ? "APPROVE" :
    decision === "changes" ? "REQUEST_CHANGES" :
    "COMMENT"
  );

  async function submitReview() {
    submitting = true;
    try {
      // Sync local diff with remote before bundling inline comments into the review.
      await app.cmd("force_refresh_diff", {});
      await app.cmd("submit_github_review", { mode: ghEvent, summary });
      pushMode = null;
      summary = "";
    } finally {
      submitting = false;
    }
  }

  async function submitIndividual() {
    submitting = true;
    await app.cmd("push_github_comments");
    submitting = false;
    pushMode = null;
  }
</script>

<Card>
  <div class="flex items-center justify-between mb-3">
    <SectionLabel>Comments</SectionLabel>
    <div class="flex items-center gap-2">
      <span class="flex items-center gap-1 text-[10px] mono text-comment"><svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>{ai.local_comment_count} local</span>
      {#if ai.github_comment_count > 0}
        <span class="flex items-center gap-1 text-[10px] mono text-muted">{ai.github_comment_count} GitHub</span>
      {/if}
      {#if annotationCount > 0}
        <button
          type="button"
          onclick={scrollToAnnotations}
          class="text-[10px] mono text-muted hover:text-fg-2 px-1 py-0.5 rounded border border-hairline"
          title="Jump to UI annotations"
        >+ {annotationCount} annotation{annotationCount === 1 ? "" : "s"}</button>
      {/if}
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-1.5 mb-3">
    <button
      type="button"
      onclick={() => app.setCommentVisibility({ hideOutdated: !app.commentVisibility.hideOutdated })}
      class="px-2 py-1 rounded text-[10px] border {app.commentVisibility.hideOutdated ? 'bg-hover border-border text-fg' : 'border-hairline text-muted hover:text-fg-2'}"
      title="Hide outdated GitHub comments in the side panel and inline diff"
    >Hide outdated</button>
    <button
      type="button"
      onclick={() => app.setCommentVisibility({ hideResolved: !app.commentVisibility.hideResolved })}
      class="px-2 py-1 rounded text-[10px] border {app.commentVisibility.hideResolved ? 'bg-hover border-border text-fg' : 'border-hairline text-muted hover:text-fg-2'}"
      title="Hide resolved GitHub comments in the side panel and inline diff"
    >Hide resolved</button>
    <button
      type="button"
      onclick={() => app.setCommentVisibility({ hideAll: !app.commentVisibility.hideAll })}
      class="px-2 py-1 rounded text-[10px] border {app.commentVisibility.hideAll ? 'bg-del-bg border-del-fg/30 text-del-fg' : 'border-hairline text-muted hover:text-fg-2'}"
      title="Hide every GitHub comment in the side panel and inline diff"
    >Hide all</button>
    {#if visibleCommentThreads.length !== commentThreads.length}
      <span class="text-[10px] text-muted mono">{visibleCommentThreads.length}/{commentThreads.length} shown</span>
    {/if}
  </div>

  <div class="space-y-2">
    {#each visibleCommentThreads as thread (thread.id)}
      <InlineThread {thread} hunk_idx={0} variant="panel" />
    {/each}
  </div>

  <!-- Push group -->
  <div class="mt-4 border-t border-hairline pt-3">
    <div class="flex items-center justify-between mb-2">
      <SectionLabel size="sm">Push to GitHub</SectionLabel>
      <span class="text-[10px] mono text-muted">{ai.unpushed} unpushed</span>
    </div>

    {#if pushMode === null}
      <div class="grid grid-cols-2 gap-2">
        <button
          onclick={() => pushMode = "review"}
          title="Backend doesn't yet accept summary/decision — currently pushes the same as Individually"
          class="px-3 py-2 rounded-md border border-border hover:border-accent hover:bg-hover text-left transition"
        >
          <div class="flex items-center gap-1.5 text-sm text-fg mb-0.5">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
            As review
            <span class="ml-auto text-[9px] mono text-muted uppercase tracking-wider">soon</span>
          </div>
          <div class="text-[10px] text-muted">Summary + decision · single approval gate</div>
        </button>
        <button
          onclick={() => pushMode = "individual"}
          class="px-3 py-2 rounded-md border border-border hover:border-accent hover:bg-hover text-left transition"
        >
          <div class="flex items-center gap-1.5 text-sm text-fg mb-0.5">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
            Individually
          </div>
          <div class="text-[10px] text-muted">Each comment posts standalone · no review</div>
        </button>
      </div>
    {/if}

    {#if pushMode === "review"}
      <div class="rounded-lg border border-border bg-surface">
        <div class="px-3 py-2 border-b border-hairline flex items-center gap-2 text-xs">
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-add-fg"><path d="M9 11l3 3L22 4"/></svg>
          <span class="text-fg-2 font-medium">Push as review</span>
          <span class="text-muted">· {commentThreads.length} comment{commentThreads.length === 1 ? "" : "s"}</span>
          <button onclick={() => pushMode = null} aria-label="Cancel push" title="Cancel" class="ml-auto text-muted hover:text-fg-2">
            <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
          </button>
        </div>

        <textarea
          bind:value={summary}
          rows="3"
          class="w-full bg-transparent text-sm px-3 py-2 outline-none resize-none font-sans placeholder:text-muted"
          placeholder={`Overall review comment (optional)… e.g. "Looks great overall. One typing question on the new options API."`}
        ></textarea>

        <div class="px-3 py-2 border-t border-hairline">
          <SectionLabel size="sm">Decision</SectionLabel>
          <div class="grid grid-cols-3 gap-1 mt-1.5">
            <button
              onclick={() => decision = "comment"}
              class="px-2 py-1.5 rounded text-[11px] flex flex-col items-center gap-0.5 transition {decision === 'comment' ? 'bg-hover text-fg ring-1 ring-border' : 'text-fg-3 hover:bg-card'}"
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
              Comment
            </button>
            <button
              onclick={() => decision = "approve"}
              class="px-2 py-1.5 rounded text-[11px] flex flex-col items-center gap-0.5 transition {decision === 'approve' ? 'bg-add-bg text-add-fg ring-1 ring-add-fg/40' : 'text-fg-3 hover:bg-card'}"
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="20 6 9 17 4 12"/></svg>
              Approve
            </button>
            <button
              onclick={() => decision = "changes"}
              class="px-2 py-1.5 rounded text-[11px] flex flex-col items-center gap-0.5 transition {decision === 'changes' ? 'bg-del-bg text-del-fg ring-1 ring-del-fg/40' : 'text-fg-3 hover:bg-card'}"
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 12c0-5 4-9 9-9s9 4 9 9-4 9-9 9-9-4-9-9z"/><path d="M9 9h6v6"/></svg>
              Request changes
            </button>
          </div>
        </div>

        <div class="px-3 py-2 border-t border-hairline flex items-center gap-2">
          <span class="text-[10px] mono text-muted">questions stay local</span>
          <button
            onclick={submitReview}
            disabled={submitting}
            class="ml-auto px-3 py-1.5 rounded-md text-xs font-medium text-black disabled:opacity-50 disabled:cursor-not-allowed {decision === 'approve' ? 'bg-add-fg hover:opacity-90' : decision === 'changes' ? 'bg-del-fg hover:opacity-90' : 'bg-accent hover:opacity-90'}"
          >
            {#if submitting}Submitting…{:else}{decision === "approve" ? "Submit approval" : decision === "changes" ? "Submit changes request" : "Submit review"}{/if}
          </button>
        </div>
      </div>
    {/if}

    {#if pushMode === "individual"}
      <div class="rounded-lg border border-border bg-surface p-3">
        <div class="flex items-start gap-2 mb-3">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="mt-0.5 shrink-0 text-ai"><circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/></svg>
          <div class="text-sm text-fg-2 leading-snug">Push <span class="text-fg font-medium">{commentThreads.length} comment{commentThreads.length === 1 ? "" : "s"}</span> as a standalone GitHub comment? It won't be tied to a review submission.</div>
        </div>
        <div class="flex items-center gap-2">
          <button onclick={() => pushMode = null} disabled={submitting} class="px-3 py-1.5 rounded-md text-xs text-fg-2 hover:bg-hover disabled:opacity-50 disabled:cursor-not-allowed">Cancel</button>
          <button onclick={submitIndividual} disabled={submitting} class="ml-auto px-3 py-1.5 rounded-md text-xs font-medium bg-accent hover:opacity-90 text-black disabled:opacity-50 disabled:cursor-not-allowed">{submitting ? "Pushing…" : "Push"}</button>
        </div>
      </div>
    {/if}
  </div>

  <button
    onclick={openExportModal}
    class="mt-3 w-full px-3 py-1.5 text-xs rounded-md border border-border hover:bg-hover text-fg-2 flex items-center justify-center gap-2"
  >
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M12 5l7 7-7 7"/></svg>
    Export to coding agent
  </button>
</Card>
