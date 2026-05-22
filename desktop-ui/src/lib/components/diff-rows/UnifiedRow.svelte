<script lang="ts">
  import { diffSel } from "$lib/stores/diffSelection.svelte";
  import { wordDiff } from "$lib/wordDiff";
  import type { CrossFileFlatRow } from "$lib/diffRenderModel";
  import type { LineSnapshot } from "$lib/types";

  interface Props {
    row: Extract<CrossFileFlatRow, { type: "content-unified" }>;
    line: LineSnapshot;
    partner: LineSnapshot | null;
    filePath: string;
  }
  const { row, line, partner, filePath }: Props = $props();

  const spanColorRemap: Record<string, string> = {
    // OneHalfDark colors that need adjustment on our dark bg
    "#5c6370": "#a7b1ba",  // OneHalfDark comment → readable gray
    "#98c379": "#d4f0e4",  // OneHalfDark green string on add-bg → light teal
    // Ocean Dark fallbacks (kept for safety)
    "#4f5b66": "#a7b1ba", "#343d46": "#a7b1ba", "#65737e": "#a7b1ba",
    "#6b6b6b": "#a7b1ba", "#5e5e5e": "#a7b1ba",
    "#99c794": "#d4f0e4", "#a3be8c": "#d4f0e4",
  };
  function remapColor(c: string): string {
    return c ? (spanColorRemap[c.toLowerCase()] ?? c) : c;
  }

  function lineClass(kind: string) {
    if (kind === "add") return "diff-add";
    if (kind === "del") return "diff-del";
    return "";
  }

  const ln = $derived(line.new_num ?? line.old_num);

  const wdU = $derived.by(() => {
    if (!partner) return null;
    if (line.kind === "del") return wordDiff(line.text, partner.text);
    if (line.kind === "add") return wordDiff(partner.text, line.text);
    return null;
  });
  const wdSpans = $derived(wdU ? (line.kind === "del" ? wdU.old : wdU.new) : null);
  const wdBg = $derived(line.kind === "del" ? "bg-del-fg/30" : "bg-add-fg/30");

  const isSelected = $derived(ln !== null && diffSel.file === filePath && diffSel.sel(ln));

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
  class="grid grid-cols-[40px_minmax(0,1fr)] diff-row {lineClass(line.kind)} {isSelected ? 'is-selected' : ''}"
  style="height:{row.height}px"
  data-row-identity={row.identity}
  onmouseenter={() => { if (ln !== null && diffSel.file === filePath) diffSel.extend(ln); }}
>
  <div class="leading-6 text-right pr-2 gutter {lineClass(line.kind)} {isSelected ? 'is-selected' : ''}">
    {line.kind === "del" ? (line.old_num ?? "") : (line.new_num ?? line.old_num ?? "")}
    {#if ln !== null && line.kind !== "fold"}
      <button
        class="add-comment-btn"
        onmousedown={(e) => diffSel.begin(ln, e.shiftKey, e, filePath)}
      >+</button>
    {/if}
  </div>
  <div class="leading-6 pr-3 whitespace-pre break-all {lineClass(line.kind)} {isSelected ? 'is-selected' : ''}" style={leadingWS()}>
    {#if line.kind === "add"}
      <span class="text-add-fg">+</span>
    {:else if line.kind === "del"}
      <span class="text-del-fg">-</span>
    {:else}
      <span>&nbsp;</span>
    {/if}
    {#if wdSpans}
      {#each wdSpans as wspan}
        {#if wspan.changed}
          <span class={wdBg}>{wspan.text}</span>
        {:else}
          {wspan.text}
        {/if}
      {/each}
    {:else if line.spans}
      {#each line.spans as span}
        {#if span.color}
          <span style="color: {remapColor(span.color)}">{span.text}</span>
        {:else}
          {span.text}
        {/if}
      {/each}
    {:else}
      {line.text}
    {/if}
  </div>
</div>
