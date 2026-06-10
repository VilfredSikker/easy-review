<script lang="ts">
  import { lineHasAnchorRangeHighlight, type AnnotationIndex } from "$lib/diffAnnotations";
  import type { CommentVisibility } from "$lib/diffAnnotations";
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { refHighlight } from "$lib/stores/referenceHighlight.svelte";
  import { caretTextOffset, identifierAt } from "$lib/referenceHighlight";
  import { wordDiff } from "$lib/wordDiff";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import DiffLineContent from "./DiffLineContent.svelte";
  import type { LineSnapshot } from "$lib/types";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "content-unified" }>;
    line: LineSnapshot;
    partner: LineSnapshot | null;
    filePath: string;
    rowIdx: number;
    annotationIndex: AnnotationIndex;
    commentVisibility: CommentVisibility;
  }
  const { row, line, partner, filePath, rowIdx, annotationIndex, commentVisibility }: Props =
    $props();

  function lineClass(kind: string) {
    if (kind === "add") return "diff-add";
    if (kind === "del") return "diff-del";
    return "";
  }

  const ln = $derived(line.new_num ?? line.old_num);
  const side = $derived(line.kind === "del" ? "old" : "new");

  const wdU = $derived.by(() => {
    if (!partner) return null;
    if (line.kind === "del") return wordDiff(line.text, partner.text);
    if (line.kind === "add") return wordDiff(partner.text, line.text);
    return null;
  });
  const wdSpans = $derived(wdU ? (line.kind === "del" ? wdU.old : wdU.new) : null);
  const wdBg = $derived(line.kind === "del" ? "bg-del-fg/30" : "bg-add-fg/30");

  const isSelected = $derived(ln !== null && diffSel.file === filePath && diffSel.sel(ln, side));
  const isAnchorRange = $derived(
    ln !== null &&
      lineHasAnchorRangeHighlight(annotationIndex, filePath, ln, side, commentVisibility),
  );

  /**
   * Reference highlight (issue #69): click an identifier to highlight all
   * occurrences across the rendered diff; click it again (or a non-identifier
   * spot, or press Escape) to clear. Skipped when the user is selecting text.
   * The code div renders a 1-char marker (+/-/nbsp) before the line text, so
   * the caret offset is shifted by 1.
   */
  function onCodeClick(e: MouseEvent) {
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed) return;
    const caret = caretTextOffset(e, e.currentTarget as HTMLElement);
    const ident = caret === null ? null : identifierAt(line.text, caret - 1);
    refHighlight.toggle(ident);
  }

  function leadingWS(): string {
    const t = line.text;
    let n = 0;
    while (n < t.length && (t[n] === " " || t[n] === "\t")) n++;
    const cols = n + 2;
    return `padding-left: calc(0.75rem + ${cols}ch); text-indent: -${cols}ch;`;
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="grid grid-cols-[40px_minmax(0,1fr)] diff-row {lineClass(line.kind)} {isSelected ? 'is-selected' : ''} {isAnchorRange ? 'is-anchor-range' : ''}"
  style="height:{row.height}px"
  data-row-identity={row.identity}
  data-row-idx={rowIdx}
>
  <div class="leading-6 text-right pr-2 gutter {lineClass(line.kind)} {isSelected ? 'is-selected' : ''}">
    {line.kind === "del" ? (line.old_num ?? "") : (line.new_num ?? line.old_num ?? "")}
    {#if ln !== null && line.kind !== "fold"}
      <button
        class="add-comment-btn"
        onmousedown={(e) => diffSel.begin(ln, e.shiftKey, e, filePath, side, rowIdx)}
      >+</button>
    {/if}
  </div>
  <div class="leading-6 pr-3 whitespace-pre break-all {lineClass(line.kind)} {isSelected ? 'is-selected' : ''}" style={leadingWS()} onclick={onCodeClick}>
    {#if line.kind === "add"}
      <span class="text-add-fg">+</span>
    {:else if line.kind === "del"}
      <span class="text-del-fg">-</span>
    {:else}
      <span>&nbsp;</span>
    {/if}
    <DiffLineContent
      text={line.text}
      wordSpans={wdSpans}
      syntaxSpans={line.spans}
      changedBgClass={wdBg}
    />
  </div>
</div>
