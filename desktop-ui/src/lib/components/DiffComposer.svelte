<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import type { DiffViewMode } from "$lib/stores/app.svelte";

  interface Props {
    /** Absolute top position in px. When set, renders absolute (flat mode); otherwise sticky. */
    topPx?: number;
    viewMode?: DiffViewMode;
  }
  const { topPx, viewMode = "unified" }: Props = $props();

  const gutterInsetPx = $derived(viewMode === "split" ? 80 : 40);

  const canSubmit = $derived(diffSel.text.trim().length > 0);
  let composerEl: HTMLTextAreaElement | null = $state(null);
  let didFocusForSelection = $state(false);

  $effect(() => {
    if (!diffSel.composerOpen) {
      didFocusForSelection = false;
      return;
    }
    if (didFocusForSelection || !composerEl) return;
    didFocusForSelection = true;
    queueMicrotask(() => composerEl?.focus());
  });

  /**
   * Find which hunk the selected line range belongs to. Old-side selections
   * need old ranges; new-side selections use new ranges.
   */
  function findHunkIdx(): number {
    const snap = app.snapshot;
    if (!snap || diffSel.file === null || diffSel.start === null) return 0;
    const file = snap.files.find((f) => f.path === diffSel.file);
    if (!file) return 0;
    const ln = diffSel.first();
    const idx = file.hunks.findIndex((h) => {
      if (diffSel.side === "old") {
        return ln >= h.old_start && ln < h.old_start + h.old_count;
      }
      return ln >= h.new_start && ln < h.new_start + h.new_count;
    });
    return idx === -1 ? 0 : idx;
  }

  async function submit() {
    if (!canSubmit || diffSel.file === null || diffSel.start === null) return;
    const command = diffSel.kind === "comment" ? "add_comment" : "add_question";
    const lineStart = diffSel.first();
    const lineEnd = diffSel.last();
    const cmdArgs: Record<string, unknown> = {
      file: diffSel.file,
      hunkIdx: findHunkIdx(),
      lineNum: lineStart,
      lineNumEnd: lineStart !== lineEnd ? lineEnd : null,
      text: diffSel.text.trim(),
    };
    if (command === "add_comment" && diffSel.side !== null) {
      cmdArgs.side = diffSel.side === "old" ? "LEFT" : "RIGHT";
    }
    await app.cmd(command, cmdArgs);
    diffSel.clear();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      diffSel.clear();
    } else if (e.ctrlKey && (e.key === "t" || e.key === "T")) {
      e.preventDefault();
      diffSel.kind = diffSel.kind === "comment" ? "question" : "comment";
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  }
</script>

<div
  onclick={(e) => e.stopPropagation()}
  role="dialog"
  aria-label="Add comment or question"
  tabindex="-1"
  onkeydown={() => {}}
  style={topPx !== undefined
    ? `position:absolute;top:${topPx}px;left:calc(${gutterInsetPx}px + 0.75rem);right:1rem;z-index:20`
    : undefined}
  class="{topPx === undefined ? 'sticky bottom-0 left-0 right-0 mx-4 mb-4 mt-2' : 'mb-4 mt-2'} rounded-lg overflow-hidden font-sans shadow-[0_20px_40px_-8px_rgba(0,0,0,0.7),0_0_0_1px_rgba(255,255,255,0.04)]
         {diffSel.kind === 'question'
           ? 'border border-question/40 bg-card'
           : 'border border-blue-500/40 bg-card'}"
>
  <!-- Header -->
  <div class="px-3 py-2 border-b border-hairline flex items-center gap-2 text-xs">
    {#if diffSel.kind === "comment"}
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="#3b82f6" stroke-width="2.5"><path d="M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z"/></svg>
    {/if}
    <span class="text-fg-2 font-medium">{diffSel.rangeLabel()}</span>

    <div class="ml-3 flex items-center gap-0.5 bg-bg border border-hairline rounded-md p-0.5">
      <button
        onclick={() => (diffSel.kind = "comment")}
        class="px-2 py-0.5 rounded text-[11px] flex items-center gap-1 transition
               {diffSel.kind === 'comment' ? 'bg-comment text-black font-medium' : 'text-fg-3 hover:text-fg-2'}"
      >
        <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        Comment
      </button>
      <button
        onclick={() => (diffSel.kind = "question")}
        class="px-2 py-0.5 rounded text-[11px] flex items-center gap-1 transition
               {diffSel.kind === 'question' ? 'bg-question text-black font-medium' : 'text-fg-3 hover:text-fg-2'}"
      >
        <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>
        Question
      </button>
    </div>

    <span class="ml-auto text-[10px] mono text-muted">
      {diffSel.kind === "question" ? "private · won't push" : "will sync to GitHub"}
    </span>
    <button onclick={() => diffSel.clear()} aria-label="Cancel" class="ml-2 text-muted hover:text-fg-2">
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6L6 18M6 6l12 12"/></svg>
    </button>
  </div>

  <textarea
    bind:this={composerEl}
    bind:value={diffSel.text}
    onkeydown={handleKeydown}
    rows="3"
    placeholder={diffSel.kind === "question"
      ? "Ask a question about these lines… (only you see this)"
      : "Add a review comment…"}
    class="w-full bg-transparent text-sm px-3 py-2.5 outline-none resize-none font-sans placeholder:text-muted leading-relaxed"
  ></textarea>

  <div class="px-3 py-2 border-t border-hairline flex items-center gap-2 text-[11px]">
    <span class="text-muted">Markdown supported</span>
    <span class="ml-auto text-muted flex items-center gap-1">
      <span class="kbd">ctrl+t</span> toggle
      <span class="kbd">esc</span> cancel
    </span>
    <button
      onclick={submit}
      disabled={!canSubmit}
      class="px-3 py-1.5 rounded-md text-xs font-medium transition disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-1.5
             {diffSel.kind === 'question'
               ? 'bg-question hover:bg-question/90 text-black'
               : 'bg-comment hover:bg-comment/90 text-black'}"
    >
      <span>{diffSel.kind === "question" ? "Save question" : "Add comment"}</span>
      <span class="opacity-60 mono">⌘⏎</span>
    </button>
  </div>
</div>
