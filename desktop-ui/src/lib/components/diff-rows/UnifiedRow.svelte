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
  import type { LineSnapshot } from "$lib/types";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "content-unified" }>;
    line: LineSnapshot;
    partner: LineSnapshot | null;
    filePath: string;
    rowIdx: number;
    annotationIndex: AnnotationIndex;
    commentVisibility: CommentVisibility;
    /** Cell column capacity when word wrap is on; null = no wrap (in-panel pan). */
    wrapCols?: number | null;
    /** Mark lines colour of the commit behind this line; null leaves the add/del bar. */
    commitColor?: string | null;
  }
  const {
    row,
    line,
    partner,
    filePath,
    rowIdx,
    annotationIndex,
    commentVisibility,
    wrapCols = null,
    commitColor = null,
  }: Props = $props();

  const wrap = $derived(wrapCols !== null);

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
  const wdSpans = $derived.by(() => {
    if (!wdU) return null;
    return line.kind === "del" ? wdU.old : wdU.new;
  });
  const wdBg = $derived(line.kind === "del" ? "wd-change-del" : "wd-change-add");
  const bgKind = $derived(diffBgKind(line.kind));

  const isSelected = $derived(ln !== null && diffSel.file === filePath && diffSel.sel(ln, side));
  const isAnchorRange = $derived(
    ln !== null &&
      lineHasAnchorRangeHighlight(annotationIndex, filePath, ln, side, commentVisibility),
  );

  /**
   * Reference highlight: click an identifier to highlight all
   * occurrences across the rendered diff; click it again (or a non-identifier
   * spot, or press Escape) to clear. Skipped when the user is selecting text.
   * Cmd+click (Ctrl+click on non-mac) also opens the usages popover at the
   * click point. The code div renders a 1-char marker (+/-/nbsp) before the
   * line text, so the caret offset is shifted by 1.
   */
  function onCodeClick(e: MouseEvent) {
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed) return;
    const caret = caretTextOffset(e, e.currentTarget as HTMLElement);
    const ident = caret === null ? null : identifierAt(line.text, caret - 1);
    if (e.metaKey || e.ctrlKey) {
      refHighlight.openUsages(ident, { x: e.clientX, y: e.clientY });
    } else {
      refHighlight.toggle(ident);
    }
  }

</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="grid grid-cols-[var(--er-split-gutter)_minmax(0,1fr)] diff-row {lineClass(line.kind)} {isSelected ? 'is-selected' : ''} {isAnchorRange ? 'is-anchor-range' : ''}"
  style={wrap ? "min-height:24px" : `height:${row.height}px`}
  data-row-identity={row.identity}
  data-row-idx={rowIdx}
>
  <div
    class="leading-6 text-right gutter {lineClass(line.kind)} {isSelected ? 'is-selected' : ''} {commitColor ? 'commit-mark' : ''}"
    style={commitColor ? `--cc:${commitColor}` : undefined}
  >
    <!-- The number is the click target (prototype `lnCell`): no hover-only "+"
         to aim for, and shift-click extends the range from the same cell. -->
    {#if ln !== null && line.kind !== "fold"}
      <button type="button"
        class="block w-full pr-2 text-right cursor-pointer {isSelected ? 'text-accent' : 'hover:text-accent'}"
        aria-label="Comment on line {ln}"
        onmousedown={(e) => diffSel.begin(ln, e.shiftKey, e, { file: filePath, side, startRowIdx: rowIdx })}
      >{line.kind === "del" ? (line.old_num ?? "") : (line.new_num ?? line.old_num ?? "")}</button>
    {:else}
      <span class="pr-2">{line.kind === "del" ? (line.old_num ?? "") : (line.new_num ?? line.old_num ?? "")}</span>
    {/if}
  </div>
  <div class="leading-6 pr-3 {wrap ? 'whitespace-pre-wrap break-all' : 'whitespace-pre'} {lineClass(line.kind)} {isSelected ? 'is-selected' : ''}" onclick={onCodeClick}>
    <div class={wrap ? "" : "pan-l"} style={hangingIndentStyle(line.text, wrapCols)}>
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
        kind={bgKind}
      />
    </div>
  </div>
</div>

<style>
  /* Mark lines: the gutter's add/del bar takes the colour of the commit that
     introduced (or removed) the line. A selection keeps its own bar. */
  .diff-row:not(.is-selected) > .gutter.commit-mark {
    box-shadow: inset 3px 0 0 0 var(--cc);
  }
</style>
