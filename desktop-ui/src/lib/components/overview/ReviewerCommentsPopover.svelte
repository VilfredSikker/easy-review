<script lang="ts">
  import type { GhCommentSummary, GhReviewSummary, GhReviewerSummary, ThreadSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { navigateToThread } from "$lib/dom";
  import { rightRail } from "$lib/stores/rightRail.svelte";
  import { rightPanelTab } from "$lib/stores/rightPanelTab.svelte";
  import { rightPanelFilters } from "$lib/components/rpanel/panelFilters.svelte";
  import { threadExcerpt } from "$lib/components/rpanel/commentsPanel";
  import { shortAge } from "$lib/relativeTime";
  import {
    entryWhere,
    reviewerEntries,
    reviewerStateLabel,
    reviewerStateTone,
    type ReviewerEntry,
  } from "$lib/reviewerComments";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  /**
   * A reviewer row's status, as a button counting what they left; it opens a
   * popover listing each entry with a jump to its line.
   */
  interface Props {
    reviewer: GhReviewerSummary;
    /** Full name for the popover header; the login otherwise. */
    displayName?: string;
    reviews?: readonly GhReviewSummary[];
    comments?: readonly GhCommentSummary[];
    threads?: readonly ThreadSnapshot[] | null;
    onJump?: (entry: ReviewerEntry, thread: ThreadSnapshot | null) => void;
    onShowInPanel?: (reviewer: GhReviewerSummary) => void;
  }

  const { reviewer, displayName, reviews = [], comments = [], threads = [], onJump, onShowInPanel }: Props = $props();

  const entries = $derived(reviewerEntries(reviewer.reviewer, { reviews, comments, threads }));
  const label = $derived(reviewerStateLabel(reviewer));
  const tone = $derived(reviewerStateTone(reviewer.state));
  const TONE_CLASS = { ok: "text-add-fg", block: "text-del-fg", neutral: "text-fg-3" } as const;
  const toneClass = $derived(TONE_CLASS[tone]);

  const name = $derived(displayName ?? reviewer.reviewer);
  const initials = $derived(
    name
      .split(/[\s_-]+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0].toUpperCase())
      .join(""),
  );

  let open = $state(false);
  let anchor = $state<{ top: number; right: number } | null>(null);
  let button = $state<HTMLButtonElement | null>(null);

  const WIDTH = 360;
  const EDGE = 8;
  const panelStyle = $derived.by(() => {
    const top = anchor?.top ?? 48;
    const vw = typeof window === "undefined" ? WIDTH + 2 * EDGE : window.innerWidth;
    const left = Math.max(EDGE, Math.min((anchor?.right ?? vw - EDGE) - WIDTH, vw - WIDTH - EDGE));
    return `top: ${top}px; left: ${left}px; width: min(${WIDTH}px, calc(100vw - ${2 * EDGE}px)); max-height: calc(100vh - ${top + 16}px)`;
  });

  function toggle() {
    if (open) {
      open = false;
      return;
    }
    const r = button?.getBoundingClientRect();
    anchor = r ? { top: r.bottom + 6, right: r.right } : null;
    open = true;
  }

  function threadFor(entry: ReviewerEntry): ThreadSnapshot | null {
    return entry.threadId ? (threads ?? []).find((t) => t.id === entry.threadId) ?? null : null;
  }

  function jump(entry: ReviewerEntry) {
    open = false;
    const thread = threadFor(entry);
    if (onJump) {
      onJump(entry, thread);
      return;
    }
    if (!thread) return;
    app.setMainView("diff");
    void navigateToThread(thread);
  }

  function showInPanel() {
    open = false;
    if (onShowInPanel) {
      onShowInPanel(reviewer);
      return;
    }
    app.setMainView("diff");
    rightRail.expand();
    rightPanelTab.set("comments");
    rightPanelFilters.setKind("comment");
  }
</script>

{#if entries.length > 0}
  <button
    type="button"
    bind:this={button}
    class="ml-auto shrink-0 rounded px-1 py-px text-[11px] {toneClass} hover:bg-hover hover:text-fg hover:underline"
    aria-haspopup="dialog"
    aria-expanded={open}
    title="What {name} left on this PR"
    onclick={toggle}
  >{label ? `${label} · ` : ""}{entries.length}</button>
{:else if label}
  <span class="ml-auto shrink-0 px-1 text-[11px] {toneClass}">{label}</span>
{/if}

<ModalShell
  {open}
  ariaLabel="Reviewer comments"
  onClose={() => (open = false)}
  backdropClass="fixed inset-0 z-[39]"
  backdropStyle=""
  panelClass="z-[40] grid grid-rows-[auto_minmax(0,1fr)_auto] overflow-hidden rounded-xl border border-border bg-card shadow-[0_18px_44px_rgba(0,0,0,.32)] outline-none"
  {panelStyle}
  dataModal="reviewer-comments"
>
  <div class="flex items-center gap-2 border-b border-hairline py-2.5 pl-3.5 pr-3">
    <span class="grid h-[22px] w-[22px] shrink-0 place-items-center rounded-full bg-hover text-[10px] font-semibold text-fg-2" aria-hidden="true">{initials}</span>
    <b class="min-w-0 truncate text-[13px] font-semibold">{name}</b>
    <span class="shrink-0 text-[11.5px] text-muted">{entries.length} on this PR</span>
    <button
      type="button"
      class="ml-auto grid h-[22px] w-[22px] shrink-0 place-items-center rounded-[5px] text-[13px] text-fg-3 hover:bg-hover hover:text-fg"
      aria-label="Close"
      onclick={() => (open = false)}
    >✕</button>
  </div>
  <div class="grid content-start gap-0.5 overflow-auto p-1.5">
    {#each entries as entry (entry.key)}
      {@const jumps = entry.threadId !== null}
      <button
        type="button"
        class="grid gap-0.5 rounded-[7px] px-2 py-[7px] text-left {jumps ? 'hover:bg-hover' : 'cursor-default'}"
        disabled={!jumps}
        title={jumps ? `Jump to ${entry.file}:${entry.line}` : undefined}
        onclick={() => jump(entry)}
      >
        <span class="font-mono text-[10.5px] text-muted">
          {entryWhere(entry)}{entry.kind === "reply" ? " · reply" : ""}{entry.at ? ` · ${shortAge(entry.at)}` : ""}
        </span>
        <span class="line-clamp-2 text-[12.5px] leading-[1.45] text-fg">{threadExcerpt(entry.text, 240)}</span>
      </button>
    {/each}
  </div>
  <div class="flex items-center gap-2 border-t border-hairline bg-bg px-3.5 py-2.5">
    <span class="flex-1"></span>
    <button
      type="button"
      class="inline-flex h-7 items-center rounded-[7px] px-2.5 text-[12px] font-medium text-fg-2 hover:bg-hover hover:text-fg"
      onclick={showInPanel}
    >Show in Comments panel</button>
  </div>
</ModalShell>
