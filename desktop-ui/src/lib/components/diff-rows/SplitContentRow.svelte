<script lang="ts">
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { wordDiff } from "$lib/wordDiff";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import { remapSpanColor } from "$lib/spanColorRemap";
  import type { SplitRow } from "$lib/splitRows";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "content-split" }>;
    splitRow: SplitRow;
    filePath: string;
  }
  const { row, splitRow, filePath }: Props = $props();

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
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="grid grid-cols-[40px_minmax(0,1fr)_40px_minmax(0,1fr)] diff-row"
  style="height:{row.height}px"
  data-row-identity={row.identity}
>
  <!-- Left gutter -->
  <div
    class="leading-6 text-right pr-2 gutter {left ? lineClass(left.kind) : 'diff-empty'} {selLeft(leftLn) ? 'is-selected' : ''}"
  >
    {left?.old_num ?? ""}
    {#if left && leftLn !== null && left.kind !== "fold"}
      <button class="add-comment-btn" onmousedown={(e) => diffSel.begin(leftLn, e.shiftKey, e, filePath, "old")}>+</button>
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
      {#if wd}
        {#each wd.old as wspan}
          {#if wspan.changed}<span class="bg-del-fg/30">{wspan.text}</span>{:else}{wspan.text}{/if}
        {/each}
      {:else if left.spans}
        {#each left.spans as span}
          {#if span.color}<span style="color: {remapSpanColor(span.color)}">{span.text}</span>{:else}{span.text}{/if}
        {/each}
      {:else}
        {left.text}
      {/if}
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
      <button class="add-comment-btn" onmousedown={(e) => diffSel.begin(rightLn, e.shiftKey, e, filePath, "new")}>+</button>
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
      {#if wd}
        {#each wd.new as wspan}
          {#if wspan.changed}<span class="bg-add-fg/30">{wspan.text}</span>{:else}{wspan.text}{/if}
        {/each}
      {:else if right.spans}
        {#each right.spans as span}
          {#if span.color}<span style="color: {remapSpanColor(span.color)}">{span.text}</span>{:else}{span.text}{/if}
        {/each}
      {:else}
        {right.text}
      {/if}
    {:else}
      <span>&nbsp;</span>
    {/if}
  </div>
</div>
