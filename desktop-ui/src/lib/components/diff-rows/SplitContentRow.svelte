<script lang="ts">
  import { lineHasAnchorRangeHighlight, type AnnotationIndex } from "$lib/diffAnnotations";
  import type { CommentVisibility } from "$lib/diffAnnotations";
  import { diffBgKind } from "$lib/diffContrast";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
  import { caretTextOffset, identifierAt } from "$lib/referenceHighlight";
  import { wordDiff } from "$lib/wordDiff";
  import { hangingIndentStyle } from "$lib/lineWrap";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import DiffLineContent from "./DiffLineContent.svelte";
  import type { SplitRow } from "$lib/splitRows";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "content-split" }>;
    splitRow: SplitRow;
    filePath: string;
    rowIdx: number;
    annotationIndex: AnnotationIndex;
    commentVisibility: CommentVisibility;
    /** Cell column capacity when word wrap is on; null = no wrap (in-panel pan). */
    wrapCols?: number | null;
  }
  const {
    row,
    splitRow,
    filePath,
    rowIdx,
    annotationIndex,
    commentVisibility,
    wrapCols = null,
  }: Props = $props();

  const wrap = $derived(wrapCols !== null);
  const cellWs = $derived(wrap ? "whitespace-pre-wrap break-all" : "whitespace-pre");

  function lineClass(kind: string) {
    if (kind === "add") return "diff-add";
    if (kind === "del") return "diff-del";
    return "";
  }

  const left = $derived(splitRow.left);
  const right = $derived(splitRow.right);
  const leftLn = $derived(left ? (left.new_num ?? left.old_num) : null);
  const rightLn = $derived(right ? (right.new_num ?? right.old_num) : null);
  const isModifyPair = $derived(!!(left && right && left.kind === "del" && right.kind === "add"));
  const wd = $derived(isModifyPair ? wordDiff(left!.text, right!.text) : null);

  function selLeft(ln: number | null): boolean {
    if (ln === null || !diffSel.sel(ln)) return false;
    if (diffSel.file !== filePath) return false;
    if (diffSel.side !== null && diffSel.side !== "old") return false;
    return true;
  }
  function selRight(ln: number | null): boolean {
    if (ln === null || !diffSel.sel(ln)) return false;
    if (diffSel.file !== filePath) return false;
    if (diffSel.side !== null && diffSel.side !== "new") return false;
    return true;
  }

  function anchorLeft(ln: number | null): boolean {
    if (ln === null) return false;
    return lineHasAnchorRangeHighlight(annotationIndex, filePath, ln, "old", commentVisibility);
  }
  function anchorRight(ln: number | null): boolean {
    if (ln === null) return false;
    return lineHasAnchorRangeHighlight(annotationIndex, filePath, ln, "new", commentVisibility);
  }

  /**
   * Reference highlight (issue #69): click an identifier to highlight all
   * occurrences across the rendered diff; click it again (or a non-identifier
   * spot, or press Escape) to clear. Skipped when the user is selecting text.
   * Cmd+click (Ctrl+click on non-mac) also opens the usages popover at the
   * click point. Each code cell renders a 1-char marker (+/-/nbsp) before the
   * line text, so the caret offset is shifted by 1.
   */
  function onCodeClick(e: MouseEvent, lineText: string) {
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed) return;
    const caret = caretTextOffset(e, e.currentTarget as HTMLElement);
    const ident = caret === null ? null : identifierAt(lineText, caret - 1);
    if (e.metaKey || e.ctrlKey) {
      refHighlight.openUsages(ident, { x: e.clientX, y: e.clientY });
    } else {
      refHighlight.toggle(ident);
    }
  }

  const rowAnchorClass = $derived(
    anchorLeft(leftLn) || anchorRight(rightLn) ? "is-anchor-range" : "",
  );
  const rowSelClass = $derived(selLeft(leftLn) || selRight(rightLn) ? "is-selected" : "");
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="grid grid-cols-[40px_minmax(0,1fr)_40px_minmax(0,1fr)] diff-row {rowSelClass} {rowAnchorClass}"
  style={wrap ? "min-height:24px" : `height:${row.height}px`}
  data-row-identity={row.identity}
  data-row-idx={rowIdx}
>
  <!-- Left gutter -->
  <div
    class="leading-6 text-right pr-2 gutter {left ? lineClass(left.kind) : 'diff-empty'} {selLeft(leftLn) ? 'is-selected' : ''}"
  >
    {left?.old_num ?? ""}
    {#if left && leftLn !== null && left.kind !== "fold"}
      <button class="add-comment-btn" onmousedown={(e) => diffSel.begin(leftLn, e.shiftKey, e, filePath, "old", rowIdx)}>+</button>
    {/if}
  </div>
  <!-- Left code -->
  <div
    class="leading-6 pr-3 {cellWs} {left ? lineClass(left.kind) : 'diff-empty'} {selLeft(leftLn) ? 'is-selected' : ''}"
    style={left ? undefined : "padding-left: 0.75rem"}
    onclick={left ? (e) => onCodeClick(e, left.text) : undefined}
  >
    {#if left}
      <div class={wrap ? "" : "pan-l"} style={hangingIndentStyle(left.text, wrapCols)}>
        {#if left.kind === "del"}
          <span class="text-del-fg">-</span>
        {:else if left.kind === "add"}
          <span class="text-add-fg">+</span>
        {:else}
          <span>&nbsp;</span>
        {/if}
        <DiffLineContent
          text={left.text}
          wordSpans={wd?.old ?? null}
          syntaxSpans={left.spans}
          changedBgClass="wd-change-del"
          kind={diffBgKind(left.kind)}
        />
      </div>
    {:else}
      <span>&nbsp;</span>
    {/if}
  </div>
  <!-- Right gutter -->
  <div
    class="leading-6 text-right pr-2 gutter {right ? lineClass(right.kind) : 'diff-empty'} {selRight(rightLn) ? 'is-selected' : ''}"
  >
    {right?.new_num ?? ""}
    {#if right && rightLn !== null && right.kind !== "fold"}
      <button class="add-comment-btn" onmousedown={(e) => diffSel.begin(rightLn, e.shiftKey, e, filePath, "new", rowIdx)}>+</button>
    {/if}
  </div>
  <!-- Right code -->
  <div
    class="leading-6 pr-3 {cellWs} {right ? lineClass(right.kind) : 'diff-empty'} {selRight(rightLn) ? 'is-selected' : ''}"
    style={right ? undefined : "padding-left: 0.75rem"}
    onclick={right ? (e) => onCodeClick(e, right.text) : undefined}
  >
    {#if right}
      <div class={wrap ? "" : "pan-r"} style={hangingIndentStyle(right.text, wrapCols)}>
        {#if right.kind === "add"}
          <span class="text-add-fg">+</span>
        {:else if right.kind === "del"}
          <span class="text-del-fg">-</span>
        {:else}
          <span>&nbsp;</span>
        {/if}
        <DiffLineContent
          text={right.text}
          wordSpans={wd?.new ?? null}
          syntaxSpans={right.spans}
          changedBgClass="wd-change-add"
          kind={diffBgKind(right.kind)}
        />
      </div>
    {:else}
      <span>&nbsp;</span>
    {/if}
  </div>
</div>
