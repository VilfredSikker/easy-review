<script lang="ts">
  import type { ThreadSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { pendingReview } from "$lib/stores/pendingReview.svelte";
  import { reviewPage } from "$lib/stores/reviewPage.svelte";
  import { navigateToThread } from "$lib/dom";
  import { shortAge } from "$lib/relativeTime";
  import {
    REVIEW_DECISIONS,
    decisionBlockedReason,
    pendingCommentWhere,
    submitBlockedReason,
    submitFootnote,
    type PendingReviewComment,
  } from "$lib/reviewSubmit";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  /**
   * The Review button's popover: who asked for this review, the pending line
   * comments, an optional summary, and the one decision they are posted with.
   */
  interface Props {
    prNumber: number;
    /** The Review button, which the popover hangs under, right-aligned. */
    anchor: HTMLElement | null;
  }
  const { prNumber, anchor }: Props = $props();

  const snapshot = $derived(app.snapshot);
  const github = $derived(snapshot?.github ?? null);
  const review = $derived(pendingReview.review);
  const comments = $derived(review?.comments ?? []);
  const request = $derived(review?.review_request ?? null);
  const viewer = $derived(review?.viewer ?? null);
  const isReviewer = $derived(
    viewer != null &&
      (github?.reviewers ?? []).some((r) => !r.is_team && r.reviewer.toLowerCase() === viewer.toLowerCase()),
  );
  const blocked = $derived(
    submitBlockedReason(pendingReview.decision, comments.length, pendingReview.summary, github),
  );

  const WIDTH = 420;
  const EDGE = 8;
  const panelStyle = $derived.by(() => {
    const r = pendingReview.open ? anchor?.getBoundingClientRect() : undefined;
    const top = r ? r.bottom + 6 : 48;
    const vw = typeof window === "undefined" ? WIDTH + 2 * EDGE : window.innerWidth;
    const right = r?.right ?? vw - EDGE;
    const left = Math.max(EDGE, Math.min(right - WIDTH, vw - WIDTH - EDGE));
    return `top: ${top}px; left: ${left}px; width: min(${WIDTH}px, calc(100vw - ${2 * EDGE}px)); max-height: calc(100vh - ${top + 16}px)`;
  });

  function initials(name: string): string {
    return name
      .split(/[\s_-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0].toUpperCase())
      .join("");
  }

  // A pulled pending comment is drawn as `gh-<id>`; before the pull lands
  // there is no thread yet, so the jump still opens the file.
  function jump(c: PendingReviewComment) {
    pendingReview.open = false;
    const id = c.database_id != null ? `gh-${c.database_id}` : c.id;
    const drawn = snapshot?.ai?.threads.find((t) => t.id === id);
    const target = drawn ?? ({ id, file: c.path, line: c.line ?? 0 } as ThreadSnapshot);
    reviewPage.show("diff");
    void navigateToThread(target);
  }
</script>

<ModalShell
  open={pendingReview.open}
  ariaLabel="Submit review"
  onClose={() => (pendingReview.open = false)}
  backdropClass="fixed inset-0 z-[39]"
  backdropStyle=""
  panelClass="z-[40] grid grid-rows-[auto_auto_minmax(0,1fr)_auto] overflow-hidden rounded-xl border border-border bg-card shadow-[0_18px_44px_rgba(0,0,0,.32)] outline-none"
  {panelStyle}
  focusSelector="#review-summary"
  dataModal="review-submit"
>
  <div class="flex items-center gap-2 border-b border-hairline py-2.5 pl-3.5 pr-3">
    <b class="text-[13px] font-semibold">Review #{prNumber}</b>
    {#if snapshot}
      <span class="text-[11.5px] text-muted">{snapshot.reviewed_count}/{snapshot.total_count} files reviewed</span>
    {/if}
    <button
      type="button"
      class="ml-auto grid h-[22px] w-[22px] shrink-0 place-items-center rounded-[5px] text-[13px] text-fg-3 hover:bg-hover hover:text-fg"
      aria-label="Close"
      onclick={() => (pendingReview.open = false)}
    >✕</button>
  </div>

  {#if request || isReviewer}
    <div
      class="flex items-center gap-2 border-b border-hairline bg-[color-mix(in_srgb,var(--color-accent)_6%,transparent)] px-3.5 py-2.5 text-[12.5px]"
      data-testid="review-request"
    >
      {#if request}
        <span class="grid h-[20px] w-[20px] shrink-0 place-items-center rounded-full bg-hover text-[9.5px] font-semibold text-fg-2" aria-hidden="true">{initials(request.requested_by)}</span>
        <span class="min-w-0 truncate"><b>{request.requested_by}</b> requested your review · {shortAge(request.requested_at)}</span>
      {/if}
      {#if isReviewer}
        <span class="ml-auto shrink-0 rounded-full border border-border px-2 py-px text-[11px] text-fg-3">You are a reviewer</span>
      {/if}
    </div>
  {:else}
    <div></div>
  {/if}

  <div class="min-h-0 overflow-auto">
    <div class="px-3.5 pb-1 pt-2.5 text-[10.5px] font-semibold uppercase tracking-[0.06em] text-muted">
      Pending comments <span class="font-normal">{comments.length}</span>
    </div>
    {#if comments.length}
      <div class="grid max-h-[220px] gap-0.5 overflow-auto px-2" data-testid="pending-comments">
        {#each comments as c (c.id)}
          <div class="relative grid gap-0.5 rounded-[7px] py-[7px] pl-2 pr-7 hover:bg-hover">
            <button type="button" class="grid gap-0.5 text-left" title="Jump to {c.path}" onclick={() => jump(c)}>
              <span class="font-mono text-[10.5px] text-muted">{pendingCommentWhere(c)}</span>
              <span class="line-clamp-2 text-[12.5px] leading-[1.45] text-fg">{c.body}</span>
            </button>
            <button
              type="button"
              class="absolute right-1.5 top-2 grid h-[18px] w-[18px] place-items-center rounded text-[9px] text-muted hover:bg-surface hover:text-fg disabled:opacity-40"
              title="Remove from review"
              aria-label="Remove from review"
              disabled={pendingReview.busy}
              onclick={() => void pendingReview.remove(c.id)}
            >✕</button>
          </div>
        {/each}
      </div>
    {:else}
      <div class="px-3.5 pb-1.5 pt-1 text-[12px] leading-normal text-muted">
        {#if pendingReview.loading && !review}
          Reading your pending review…
        {:else if pendingReview.error}
          Could not read your pending review: {pendingReview.error}
        {:else}
          No pending comments. Add line comments with “Start a review” and they collect here until you submit.
        {/if}
      </div>
    {/if}

    <div class="px-3.5 pt-2">
      <textarea
        id="review-summary"
        bind:value={pendingReview.summary}
        rows="3"
        placeholder="Leave a summary (optional)"
        class="min-h-16 w-full resize-y rounded-[7px] border border-border bg-bg px-2.5 py-2 font-sans text-[12.5px] outline-none placeholder:text-muted focus:border-accent"
        disabled={pendingReview.busy}
      ></textarea>
    </div>

    <div class="grid gap-0.5 px-2.5 py-2" role="radiogroup" aria-label="Review decision">
      {#each REVIEW_DECISIONS as d (d.id)}
        {@const on = pendingReview.decision === d.id}
        {@const reason = decisionBlockedReason(d.id, github)}
        <label
          class={[
            "flex cursor-pointer items-start gap-2.5 rounded-[7px] px-2 py-[7px] text-[12.5px] hover:bg-hover",
            on && d.id !== "changes" && "bg-[color-mix(in_srgb,var(--color-accent)_8%,var(--color-hover))]",
            on && d.id === "changes" && "bg-[color-mix(in_srgb,var(--color-del-fg)_9%,var(--color-hover))]",
            reason && "cursor-not-allowed opacity-50",
          ]}
          title={reason ?? undefined}
        >
          <input
            type="radio"
            name="review-decision"
            value={d.id}
            class="mt-[3px] accent-[var(--color-accent)]"
            checked={on}
            disabled={reason != null}
            onchange={() => (pendingReview.decision = d.id)}
          />
          <span class="grid gap-px">
            <b class="font-semibold">{d.label}</b>
            <span class="text-[11.5px] text-muted">{reason ?? d.description}</span>
          </span>
        </label>
      {/each}
    </div>
  </div>

  <div class="flex items-center gap-2 border-t border-hairline bg-bg px-3.5 py-2.5">
    <span class="min-w-0 truncate text-[11.5px] text-muted">{submitFootnote(comments.length)}</span>
    <span class="flex-1"></span>
    <button
      type="button"
      class="inline-flex h-7 items-center rounded-[7px] px-2.5 text-[12px] font-medium text-fg-2 hover:bg-hover hover:text-fg"
      onclick={() => (pendingReview.open = false)}
    >Cancel</button>
    <button
      type="button"
      class="inline-flex h-7 items-center rounded-[7px] bg-accent px-2.5 text-[12px] font-medium text-on-accent hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
      disabled={blocked != null || pendingReview.busy}
      title={blocked ?? "Post your review to GitHub"}
      data-testid="submit-review"
      onclick={() => void pendingReview.submit()}
    >{pendingReview.busy ? "Submitting…" : "Submit review"}</button>
  </div>
</ModalShell>
