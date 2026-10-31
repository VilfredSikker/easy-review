<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { commentThreadsFromDiff, unpushedLocalCommentThreads } from "$lib/commentVisibility";
  import { navigateToThread } from "$lib/dom";
  import { baseName } from "$lib/shortFileName";
  import { copyReviewExport } from "$lib/stores/quickExport";
  import type { ExportOpts } from "$lib/exportOptions";
  import UiAnnotationsCard from "$lib/components/UiAnnotationsCard.svelte";
  import { clip } from "./findingsPanel";
  import {
    commentRows,
    KIND_FILTERS,
    KIND_LABEL,
    kindCounts,
    lineRange,
    panelThreads,
    replyTally,
    threadAudience,
    threadExcerpt,
    type ThreadKind,
  } from "./commentsPanel";
  import { rightPanelFilters } from "./panelFilters.svelte";
  import { commentsAuthorFilter } from "./commentsAuthorFilter.svelte";
  import { threadsByAuthor } from "./commentsAuthor";
  import { reviewPageTabKey } from "$lib/stores/reviewPageModel";
  import { showOverviewSection } from "$lib/components/overview/overviewNav";
  import PanelActions from "./PanelActions.svelte";

  interface Props {
    ai: AiSnapshot | null;
    currentFile: string | null;
  }

  const { ai, currentFile }: Props = $props();

  // The kind colours are theme tokens (ADR 0030): comment blue, question
  // purple, note amber. One map so the chip and the row's edge always agree.
  const KIND_TEXT: Record<ThreadKind, string> = {
    comment: "text-comment bg-comment/14",
    question: "text-question bg-question/14",
    note: "text-note bg-note/14",
  };
  const KIND_EDGE: Record<ThreadKind, string> = {
    comment: "shadow-[inset_2px_0_0_color-mix(in_srgb,var(--color-comment)_70%,transparent)]",
    question: "shadow-[inset_2px_0_0_color-mix(in_srgb,var(--color-question)_70%,transparent)]",
    note: "shadow-[inset_2px_0_0_color-mix(in_srgb,var(--color-note)_70%,transparent)]",
  };

  const files = $derived(app.snapshot?.files);
  const fileOrder = $derived((files ?? []).map((f) => f.path));

  // Set from the Overview's reviewer popover. It narrows the counts too, so the
  // chips describe the list on screen.
  const author = $derived(commentsAuthorFilter.authorFor(reviewPageTabKey(app.snapshot)));
  const threads = $derived(threadsByAuthor(panelThreads(ai?.threads, app.commentVisibility, files), author));
  // Under an author filter the gap is that filter's doing, not resolved threads.
  const hiddenGithub = $derived(
    author
      ? 0
      : commentThreadsFromDiff(ai?.threads, files).length - threads.filter((t) => t.kind === "comment").length,
  );
  const counts = $derived(kindCounts(threads));
  const rows = $derived(commentRows(threads, rightPanelFilters.kind, fileOrder));
  const pushable = $derived(unpushedLocalCommentThreads(ai?.threads, files).length);

  const pulling = $derived(app.snapshot?.bg_loading?.gh_comments ?? false);
  const freshness = $derived.by(() => {
    if (pulling) return "pulling…";
    return pushable > 0 ? `${pushable} unpushed` : "synced";
  });

  let manualPulling = $state(false);
  async function pull() {
    manualPulling = true;
    try {
      await app.cmd("pull_github_comments", { force: true });
    } finally {
      manualPulling = false;
    }
  }

  // Quick copy follows the kind filter, so what lands on the clipboard is the
  // list on screen. Annotations ride along only under All, where they show.
  function quickCopy() {
    const kind = rightPanelFilters.kind;
    const opts: ExportOpts = {
      includeComments: kind === "all" || kind === "comment",
      includeQuestions: kind === "all" || kind === "question",
      includeNotes: kind === "all" || kind === "note",
      includeFindings: false,
      includeAnnotations: kind === "all",
      includeChecklist: false,
      onlyUnresolved: false,
    };
    void copyReviewExport(opts);
  }
</script>

<div class="flex items-center justify-between gap-2 text-[12.5px] font-medium text-fg">
  <span>{counts.comment} on GitHub · {counts.question + counts.note} private</span>
  <span class="mono text-[11px] font-normal {pushable > 0 ? 'text-ai' : 'text-add-fg'}">{freshness}</span>
</div>

<div class="flex items-center gap-1 flex-wrap text-[11.5px]">
  {#each KIND_FILTERS as filter (filter.id)}
    {@const on = rightPanelFilters.kind === filter.id}
    <button
      type="button"
      aria-pressed={on}
      onclick={() => rightPanelFilters.setKind(filter.id)}
      class="px-[7px] py-px rounded-[5px] mono text-[11px]
        {on ? 'bg-hover text-fg ring-1 ring-inset ring-border' : 'text-fg-2 hover:text-fg'}"
    >{filter.label} {counts[filter.id]}</button>
  {/each}
</div>

{#if author}
  <div class="flex items-center gap-1.5 -mt-1.5 text-[11.5px] text-fg-2">
    <span>From <b class="font-semibold text-fg">{author}</b></span>
    <button
      type="button"
      onclick={() => commentsAuthorFilter.set(null, null)}
      class="rounded px-1 text-fg-3 hover:bg-hover hover:text-fg"
      aria-label="Show everyone's comments"
      title="Show everyone's comments"
    >×</button>
  </div>
{/if}

{#if hiddenGithub > 0 && rightPanelFilters.kind !== "question" && rightPanelFilters.kind !== "note"}
  <p class="m-0 -mt-1.5 text-[11px] text-fg-3">
    {hiddenGithub} resolved or outdated GitHub comment{hiddenGithub === 1 ? "" : "s"} hidden
  </p>
{/if}

<div class="grid gap-0.5 -mx-1.5">
  {#each rows as thread (thread.id)}
    {@const here = currentFile === thread.file}
    {@const tally = replyTally(thread)}
    <button
      type="button"
      onclick={() => navigateToThread(thread)}
      class="relative grid gap-[3px] px-2 py-[7px] rounded-[7px] text-left hover:bg-hover {KIND_EDGE[thread.kind]}
        {here ? 'bg-accent/8' : ''} {thread.resolved ? 'opacity-50' : ''}"
    >
      <span class="flex items-center gap-1.5 min-w-0 text-[11px] text-fg-3">
        <span class="text-[10.5px] font-semibold px-1.5 leading-[17px] rounded shrink-0 {KIND_TEXT[thread.kind]}">{KIND_LABEL[thread.kind]}</span>
        <span class="mono truncate" title={thread.file}>{clip(baseName(thread.file), 20)}:{lineRange(thread.line, thread.line_end)}</span>
        <span class="ml-auto shrink-0">{threadAudience(thread.kind)}</span>
      </span>
      <span class="text-[12.5px] leading-snug text-fg {thread.resolved ? 'line-through' : ''}">
        <b class="font-semibold">{thread.root.author}</b> {threadExcerpt(thread.root.body_markdown)}
      </span>
      {#if tally.replies > 0}
        <span class="text-[11px] text-fg-3">
          {tally.replies} {tally.replies === 1 ? "reply" : "replies"}{#if tally.ai > 0} · <span class="text-accent">✦ {tally.ai} AI</span>{/if}
        </span>
      {/if}
    </button>
  {:else}
    <p class="px-1 py-3 text-[12px] text-fg-3">Nothing here yet. Click a line number in the diff to add one.</p>
  {/each}
</div>

<PanelActions
  actions={[
    { glyph: "↻", label: "Pull GitHub comments", disabled: pulling || manualPulling, run: () => void pull() },
    ...(pushable > 0
      ? [{ glyph: "↑", label: `Push ${pushable} to GitHub…`, title: "Submit as a review or push one by one", run: () => showOverviewSection("branch") }]
      : []),
    { glyph: "⧉", label: "Quick copy export", title: "Copy this list as markdown, ready to paste into a coding agent", run: quickCopy },
    { glyph: "↗", label: "Open export…", run: () => app.setMainView("export-review") },
  ]}
/>

{#if rightPanelFilters.kind === "all"}
  <UiAnnotationsCard />
{/if}
