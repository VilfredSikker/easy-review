<script lang="ts">
  import { quoteSourceLines, threadsByBlock, type LineRange } from "$lib/documentAnchors";
  import type { DocumentBlock } from "$lib/documentPreview";
  import { app } from "$lib/stores/app.svelte";
  import type { FileSnapshot } from "$lib/types";
  import DocumentMarkdown from "./DocumentMarkdown.svelte";
  import InlineThread from "./InlineThread.svelte";
  import PreviewComposer from "./PreviewComposer.svelte";

  interface Props {
    source: string;
    blocks: readonly DocumentBlock[];
    file: FileSnapshot;
  }
  const { source, blocks, file }: Props = $props();

  /** An open composer: the highlight, its source lines, and the block it sits under. */
  let draft = $state<{ quote: string; lines: LineRange; afterBlock: number } | null>(null);
  let root: HTMLElement | null = $state(null);

  const threads = $derived(threadsByBlock(blocks, app.snapshot?.ai.threads ?? [], file.path));

  function blockOf(node: Node | null): HTMLElement | null {
    const el = node instanceof Element ? node : node?.parentElement;
    const block = el?.closest<HTMLElement>("[data-block-start]");
    return block && root?.contains(block) ? block : null;
  }

  function lineRange(el: HTMLElement): LineRange {
    return { start: Number(el.dataset.blockStart), end: Number(el.dataset.blockEnd) };
  }

  // A highlight inside the rendered document opens the composer under its last
  // block. Highlights in cards, the composer, or across them are ignored.
  function onmouseup() {
    const selection = window.getSelection();
    const quote = selection?.toString().trim() ?? "";
    if (!selection || selection.isCollapsed || !quote) return;
    const from = blockOf(selection.anchorNode);
    const to = blockOf(selection.focusNode);
    if (!from || !to) return;
    const a = lineRange(from);
    const b = lineRange(to);
    const range = { start: Math.min(a.start, b.start), end: Math.max(a.end, b.end) };
    draft = { quote, lines: quoteSourceLines(source, range, quote), afterBlock: Math.max(a.start, b.start) };
  }

  const inDraft = (block: DocumentBlock) =>
    draft !== null && block.startLine <= draft.lines.end && draft.lines.start <= block.endLine;
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="document-blocks" bind:this={root} {onmouseup}>
  {#each blocks as block (block.startLine)}
    {@const here = threads.get(block.startLine)}
    <div class="preview-block" class:selected={inDraft(block)} class:annotated={here}
      data-block-start={block.startLine} data-block-end={block.endLine}>
      <DocumentMarkdown segments={[block.segment]} />
    </div>
    {#if draft && draft.afterBlock === block.startLine}
      <PreviewComposer {file} lines={draft.lines} quote={draft.quote} onclose={() => { draft = null; }} />
    {/if}
    {#each here ?? [] as thread (thread.id)}
      <div class="preview-thread"><InlineThread {thread} hunk_idx={0} /></div>
    {/each}
  {/each}
  {#each threads.get(-1) ?? [] as thread (thread.id)}
    <div class="preview-thread"><InlineThread {thread} hunk_idx={0} /></div>
  {/each}
</div>

<style>
  .preview-block { display: flow-root; margin-left: -0.75rem; padding-left: calc(0.75rem - 2px); border-left: 2px solid transparent; }
  .preview-block.annotated { border-left-color: color-mix(in srgb, var(--color-question) 55%, transparent); }
  .preview-block.selected { border-left-color: var(--color-question); background: color-mix(in srgb, var(--color-question) 8%, transparent); }
  .preview-thread { margin: 0.25rem 0 0.9rem; }
  .document-blocks :global(.document-text) { white-space: pre-wrap; overflow-wrap: anywhere; margin: 0 0 0.8rem; font-family: "JetBrains Mono", monospace; font-size: 0.8125rem; line-height: 1.6; }
</style>
