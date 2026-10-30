<script lang="ts">
  import { untrack } from "svelte";
  import { SPLIT_ANNOTATION_TRAIL_PAD_PX } from "$lib/splitDiffLayout";
  import { app } from "$lib/stores/app.svelte";
  import { diffSel, type SelectionKind } from "$lib/stores/diffSelection.svelte";

  /**
   * How the card sits in the diff:
   * - `flow` — occupies real layout space in the row list, directly below the
   *   selected lines (the normal case).
   * - `absolute` — floating at a content offset, used when the anchor row is
   *   outside the rendered window so the draft stays alive while scrolled away.
   * - `sticky` — docked to the bottom of the diff viewport, used when the
   *   selection has no rendered row to attach to (compacted file, lazy stub).
   */
  type ComposerPlacement =
    | { kind: "flow" }
    | { kind: "absolute"; topPx: number }
    | { kind: "sticky" };

  interface Props {
    placement: ComposerPlacement;
    /** Split-view pane. Uses the same `.split-diff-grid` as posted annotation cards. */
    splitPane?: "old" | "new" | null;
  }
  const { placement, splitPane = null }: Props = $props();

  const absoluteTopPx = $derived(placement.kind === "absolute" ? placement.topPx : undefined);

  const split = $derived(splitPane !== null);
  const canSubmit = $derived(diffSel.text.trim().length > 0);
  let composerEl: HTMLTextAreaElement | null = $state(null);

  // Focus once per selection, using the store's pending flag so a remount of
  // this card never re-focuses (the browser scrolls a focused element into view,
  // which would drag the diff back to the comment). `preventScroll` leaves that
  // decision to the diff view's own one-shot scroll.
  $effect(() => {
    if (!diffSel.focusPending || !composerEl) return;
    untrack(() => {
      diffSel.focusPending = false;
    });
    queueMicrotask(() => composerEl?.focus({ preventScroll: true }));
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

  const SUBMIT_COMMAND: Record<SelectionKind, string> = {
    comment: "add_comment",
    note: "add_note",
    question: "add_question",
  };
  /** Ctrl+T cycles comment → question → note → comment. */
  const NEXT_KIND: Record<SelectionKind, SelectionKind> = {
    comment: "question",
    question: "note",
    note: "comment",
  };
  const PLACEHOLDER: Record<SelectionKind, string> = {
    question: "Ask a question about these lines… (only you see this)",
    note: "Write a note / instruction for an agent… (only you see this)",
    comment: "Add a review comment…",
  };
  const SUBMIT_LABEL: Record<SelectionKind, string> = {
    question: "Save question",
    note: "Save note",
    comment: "Add comment",
  };

  function submit() {
    if (!canSubmit || diffSel.file === null || diffSel.start === null) return;
    const command = SUBMIT_COMMAND[diffSel.kind];
    const lineStart = diffSel.first();
    const lineEnd = diffSel.last();
    const cmdArgs: Record<string, unknown> = {
      file: diffSel.file,
      hunkIdx: findHunkIdx(),
      lineNum: lineStart,
      lineNumEnd: lineStart !== lineEnd ? lineEnd : null,
      text: diffSel.text.trim(),
    };
    if (diffSel.side !== null) {
      cmdArgs.side = diffSel.side === "old" ? "LEFT" : "RIGHT";
    }
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    void app.cmd(command, cmdArgs);
    diffSel.clear();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      diffSel.clear();
    } else if (e.ctrlKey && (e.key === "t" || e.key === "T")) {
      e.preventDefault();
      diffSel.kind = NEXT_KIND[diffSel.kind];
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  }

  /** Accent bar and focus ring colour per kind, matching the posted annotation card. */
  const KIND_VAR: Record<SelectionKind, string> = {
    comment: "var(--color-comment)",
    question: "var(--color-question)",
    note: "var(--color-note)",
  };
  const KIND_LABEL: Record<SelectionKind, string> = {
    comment: "Comment",
    question: "Question",
    note: "Note",
  };
  const VISIBILITY: Record<SelectionKind, string> = {
    comment: "will sync to GitHub",
    question: "private · won't push",
    note: "private · won't push",
  };
  const KINDS: SelectionKind[] = ["comment", "question", "note"];
</script>

<div
  class={[
    placement.kind === "absolute" && "absolute left-0 right-0 z-20",
    placement.kind === "sticky" && "sticky bottom-0 left-0 right-0 z-20",
    split && ["annotation-inline-row", "annotation-split-row", "split-diff-grid"],
  ]}
  data-split-pane={split ? splitPane : undefined}
  style={absoluteTopPx !== undefined ? `top:${absoluteTopPx}px` : undefined}
>
  <div
    class={["min-w-0", split && "annotation-split-slot"]}
    style={split ? `padding-right:${SPLIT_ANNOTATION_TRAIL_PAD_PX}px` : undefined}
  >
    <div
      onclick={(e) => e.stopPropagation()}
      role="dialog"
      aria-label="Add comment or question"
      tabindex="-1"
      onkeydown={() => {}}
      class="composer-card mb-4 mt-2 grid max-w-[760px] gap-1.5 rounded-[9px] border border-border bg-card px-3 py-2.5 font-sans"
      style="--kc:{KIND_VAR[diffSel.kind]}; box-shadow: inset 3px 0 0 var(--kc)"
    >
      <div class="flex flex-wrap items-center gap-2.5">
        <div class="flex items-center gap-0.5 rounded-md border border-hairline bg-bg p-0.5" role="group" aria-label="Kind">
          {#each KINDS as k (k)}
            <button type="button"
              aria-pressed={diffSel.kind === k}
              onclick={() => (diffSel.kind = k)}
              class="rounded px-2 py-0.5 text-[11px] transition {diffSel.kind === k ? 'font-medium text-on-accent' : 'text-fg-3 hover:text-fg-2'}"
              style={diffSel.kind === k ? `background:${KIND_VAR[k]}` : undefined}
            >{KIND_LABEL[k]}</button>
          {/each}
        </div>
        <span class="mono text-[11px] text-muted">{diffSel.rangeLabel()} · {diffSel.rangeHint()}</span>
        <span class="ml-auto text-[11px] text-muted">{VISIBILITY[diffSel.kind]}</span>
      </div>

      <textarea
        bind:this={composerEl}
        bind:value={diffSel.text}
        onkeydown={handleKeydown}
        rows="3"
        placeholder={PLACEHOLDER[diffSel.kind]}
        style="--kc:{KIND_VAR[diffSel.kind]}"
        class="min-h-16 w-full resize-y rounded-[7px] border border-border bg-bg px-2.5 py-2 font-sans text-[13px] leading-normal outline-none placeholder:text-muted focus:border-[color-mix(in_srgb,var(--kc)_60%,transparent)]"
      ></textarea>

      <div class="flex flex-wrap items-center gap-2.5 text-[11px] text-muted">
        <span><span class="kbd">ctrl+t</span> toggle</span>
        <span><span class="kbd">esc</span> cancel</span>
        <span class="flex-1"></span>
        <button type="button" onclick={() => diffSel.clear()} class="rounded-md px-2.5 py-1.5 text-xs text-fg-2 hover:bg-bg">Cancel</button>
        <button type="button"
          onclick={submit}
          disabled={!canSubmit}
          class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-xs font-medium text-on-accent transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
          style="background:{KIND_VAR[diffSel.kind]}"
        >
          <span>{SUBMIT_LABEL[diffSel.kind]}</span>
          <span class="mono opacity-60">⌘⏎</span>
        </button>
      </div>
    </div>
  </div>
</div>
