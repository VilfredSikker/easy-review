<script lang="ts">
  import { documentBlocks, documentMarkdownSegments, documentPreviewKind, documentTextBlocks, type DocumentPreviewState } from "$lib/documentPreview";
  import type { FileSnapshot } from "$lib/types";
  import DocumentMarkdown from "./DocumentMarkdown.svelte";
  import DocumentPreviewBlocks from "./DocumentPreviewBlocks.svelte";

  interface Props {
    path: string;
    state: DocumentPreviewState;
    /** The reviewed file. With it, highlighting a passage opens a composer
     *  and saved questions, notes and comments show under their blocks. */
    file?: FileSnapshot;
    onretry?: () => void;
  }
  const { path, state, file, onretry }: Props = $props();
  const kind = $derived(documentPreviewKind(path));
  // Read as a boolean: every snapshot brings a new `file` object, and depending
  // on it directly would re-parse the whole document each time.
  const annotatable = $derived(file !== undefined);
  const blocks = $derived.by(() => {
    if (state.status !== "ready" || !annotatable) return [];
    return kind === "markdown" ? documentBlocks(state.text) : documentTextBlocks(state.text);
  });
  const segments = $derived(state.status === "ready" && !annotatable && kind === "markdown"
    ? documentMarkdownSegments(state.text) : []);
</script>

<div class="document-preview" data-document-preview={path}>
  {#if state.status === "loading"}
    <p class="preview-state" role="status">Loading preview…</p>
  {:else if state.status === "error"}
    <div class="preview-state" role="status">
      <p>Preview unavailable</p>
      <p class="preview-error">{state.message}</p>
      <button type="button" onclick={onretry}>Retry</button>
    </div>
  {:else if state.text.length === 0}
    <p class="preview-state">Empty file</p>
  {:else if file}
    <DocumentPreviewBlocks source={state.text} {blocks} {file} />
  {:else if kind === "markdown"}
    <DocumentMarkdown {segments} />
  {:else}
    <pre class="document-text">{state.text}</pre>
  {/if}
</div>

<style>
  .document-preview { min-width: 0; width: 100%; padding: 1rem 1.25rem; color: var(--color-fg); overflow-wrap: anywhere; }
  .preview-state { color: var(--color-fg-3); margin: 0; font-size: 0.8125rem; }
  .preview-state p { margin: 0 0 0.4rem; }
  .preview-error { white-space: pre-wrap; }
  button { padding: 0.25rem 0.6rem; border: 1px solid var(--color-border); border-radius: 4px; color: var(--color-fg); cursor: pointer; }
  .document-text { white-space: pre-wrap; overflow-wrap: anywhere; margin: 0; font-family: "JetBrains Mono", monospace; font-size: 0.8125rem; line-height: 1.6; }
</style>
