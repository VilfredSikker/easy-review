<script lang="ts">
  import { documentPreviewKind, renderDocumentMarkdown, type DocumentPreviewState } from "$lib/documentPreview";
  import { onExternalLinkClick } from "$lib/openExternalUrl";

  interface Props {
    path: string;
    state: DocumentPreviewState;
    onretry?: () => void;
  }
  const { path, state, onretry }: Props = $props();
  const html = $derived(state.status === "ready" && documentPreviewKind(path) === "markdown"
    ? renderDocumentMarkdown(state.text) : "");
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
  {:else if documentPreviewKind(path) === "markdown"}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- eslint-disable-next-line svelte/no-at-html-tags -- HTML passes through DOMPurify and a strict document formatting allowlist. -->
    <div class="document-markdown" onclick={onExternalLinkClick}>{@html html}</div>
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
  .document-markdown { font-size: 0.875rem; line-height: 1.6; }
  .document-markdown :global(h1), .document-markdown :global(h2), .document-markdown :global(h3),
  .document-markdown :global(h4), .document-markdown :global(h5), .document-markdown :global(h6) { font-weight: 600; line-height: 1.3; margin: 1rem 0 0.6rem; }
  .document-markdown :global(h1) { font-size: 1.6rem; }
  .document-markdown :global(h2) { font-size: 1.3rem; }
  .document-markdown :global(h3) { font-size: 1.1rem; }
  .document-markdown :global(p), .document-markdown :global(ul), .document-markdown :global(ol),
  .document-markdown :global(blockquote), .document-markdown :global(pre), .document-markdown :global(table) { margin: 0 0 0.8rem; }
  .document-markdown :global(ul) { list-style: disc; padding-left: 1.5rem; }
  .document-markdown :global(ol) { list-style: decimal; padding-left: 1.5rem; }
  .document-markdown :global(blockquote) { border-left: 3px solid var(--color-border); padding-left: 0.8rem; color: var(--color-fg-3); }
  .document-markdown :global(pre) { padding: 0.75rem; background: var(--color-card); border: 1px solid var(--color-border); border-radius: 4px; white-space: pre-wrap; }
  .document-markdown :global(code), .document-markdown :global(kbd) { font-family: "JetBrains Mono", monospace; font-size: 0.9em; }
  .document-markdown :global(table) { border-collapse: collapse; display: block; max-width: 100%; overflow-x: auto; }
  .document-markdown :global(td), .document-markdown :global(th) { border: 1px solid var(--color-border); padding: 0.3rem 0.65rem; text-align: left; }
  .document-markdown :global(th) { font-weight: 600; background: var(--color-card); }
  .document-markdown :global(a) { color: var(--color-action); text-decoration: underline; }
  .document-markdown :global(img) { max-width: 100%; height: auto; }
  .document-markdown :global(hr) { border: 0; border-top: 1px solid var(--color-border); margin: 1rem 0; }
  .document-markdown :global(summary) { cursor: pointer; }
</style>
