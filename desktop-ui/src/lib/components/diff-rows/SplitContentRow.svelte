<script lang="ts">
  import { lineHasAnchorRangeHighlight, type AnnotationIndex } from "$lib/diffAnnotations";
  import type { CommentVisibility } from "$lib/diffAnnotations";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { wordDiff } from "$lib/wordDiff";
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
  }
  const { row, splitRow, filePath, rowIdx, annotationIndex, commentVisibility }: Props =
    $props();

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

  function leadingWSStyle(line: { text: string }): string {
    const t = line.text;
    let n = 0;
    while (n < t.length && (t[n] === " " || t[n] === "\t")) n++;
    const cols = n + 2;
    return `padding-left: calc(0.75rem + ${cols}ch); text-indent: -${cols}ch;`;
  }

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

  const rowAnchorClass = $derived(
    anchorLeft(leftLn) || anchorRight(rightLn) ? "is-anchor-range" : "",
  );
  const rowSelClass = $derived(selLeft(leftLn) || selRight(rightLn) ? "is-selected" : "");
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="grid grid-cols-[40px_minmax(0,1fr)_40px_minmax(0,1fr)] diff-row {rowSelClass} {rowAnchorClass}"
  style="height:{row.height}px"
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
    class="leading-6 pr-3 whitespace-pre break-all {left ? lineClass(left.kind) : 'diff-empty'} {selLeft(leftLn) ? 'is-selected' : ''}"
    style={left ? leadingWSStyle(left) : "padding-left: 0.75rem"}
  >
    {#if left}
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
        changedBgClass="bg-del-fg/30"
      />
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
    class="leading-6 pr-3 whitespace-pre break-all {right ? lineClass(right.kind) : 'diff-empty'} {selRight(rightLn) ? 'is-selected' : ''}"
    style={right ? leadingWSStyle(right) : "padding-left: 0.75rem"}
  >
    {#if right}
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
        changedBgClass="bg-add-fg/30"
      />
    {:else}
      <span>&nbsp;</span>
    {/if}
  </div>
</div>
