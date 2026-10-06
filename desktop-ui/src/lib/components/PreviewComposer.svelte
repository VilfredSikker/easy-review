<script lang="ts">
  import { hunkForLines, quotedBody, type LineRange } from "$lib/documentAnchors";
  import { app } from "$lib/stores/app.svelte";
  import type { FileSnapshot } from "$lib/types";

  type Kind = "question" | "note" | "comment";

  interface Props {
    file: FileSnapshot;
    lines: LineRange;
    quote: string;
    onclose: () => void;
  }
  const { file, lines, quote, onclose }: Props = $props();

  // A GitHub comment needs a diff line; a question or note can sit anywhere.
  const hunkIdx = $derived(hunkForLines(file, lines));
  const kinds = $derived<Kind[]>(hunkIdx === null ? ["question", "note"] : ["question", "note", "comment"]);
  let picked = $state<Kind>("question");
  // A new highlight or a diff refresh can take the line off the diff; a
  // picked "comment" then falls back to a question instead of being sent where
  // it cannot land.
  const kind = $derived<Kind>(kinds.includes(picked) ? picked : "question");
  let text = $state("");
  let textarea: HTMLTextAreaElement | null = $state(null);

  $effect(() => {
    textarea?.focus({ preventScroll: true });
  });

  const LABEL: Record<Kind, string> = { question: "Question", note: "Note", comment: "Comment" };
  const PLACEHOLDER: Record<Kind, string> = {
    question: "Ask a question about this passage… (only you see this)",
    note: "Write a note / instruction for an agent… (only you see this)",
    comment: "Add a review comment…",
  };
  const ADD_COMMAND: Record<Kind, string> = { question: "add_question", note: "add_note", comment: "add_comment" };

  const rangeLabel = $derived(lines.start === lines.end ? `L${lines.start}` : `L${lines.start}–${lines.end}`);

  function submit() {
    if (!text.trim()) return;
    const body = quotedBody(quote, text);
    const lineNumEnd = lines.end !== lines.start ? lines.end : null;
    if (hunkIdx !== null) {
      if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
      void app.cmd(ADD_COMMAND[kind], {
        file: file.path, hunkIdx, lineNum: lines.start, lineNumEnd, text: body, side: "RIGHT",
      });
    } else {
      void app.cmd("add_document_thread", {
        file: file.path, kind, lineNum: lines.start, lineNumEnd, text: body, previewKey: file.preview_key ?? "",
      });
    }
    onclose();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.ctrlKey && (e.key === "t" || e.key === "T")) {
      e.preventDefault();
      picked = kinds[(kinds.indexOf(kind) + 1) % kinds.length];
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  }
</script>

<div class="preview-composer" class:public={kind === "comment"} role="dialog" aria-label="Add a question, note or comment">
  <div class="header">
    <span class="range">{rangeLabel}</span>
    <div class="kinds">
      {#each kinds as k (k)}
        <button type="button" class:active={kind === k} class:comment={k === "comment"} onclick={() => (picked = k)}>{LABEL[k]}</button>
      {/each}
    </div>
    <span class="scope">
      {#if kind === "comment"}will sync to GitHub{:else if hunkIdx === null}private · not in the diff{:else}private · won't push{/if}
    </span>
    <button type="button" class="close" aria-label="Cancel" onclick={onclose}>×</button>
  </div>
  <blockquote class="quote">{quote}</blockquote>
  <textarea bind:this={textarea} bind:value={text} {onkeydown} rows="3" placeholder={PLACEHOLDER[kind]}></textarea>
  <div class="footer">
    <span class="hint">⌘↵ save · Ctrl+T switch · Esc cancel</span>
    <button type="button" class="submit" disabled={!text.trim()} onclick={submit}>Save {LABEL[kind].toLowerCase()}</button>
  </div>
</div>

<style>
  .preview-composer { margin: 0.25rem 0 0.9rem; border: 1px solid color-mix(in srgb, var(--color-question) 40%, transparent); border-radius: 8px; background: var(--color-card); font-size: 0.8125rem; overflow: hidden; }
  .preview-composer.public { border-color: color-mix(in srgb, var(--color-action) 40%, transparent); }
  .header { display: flex; align-items: center; gap: 0.6rem; padding: 0.4rem 0.75rem; border-bottom: 1px solid var(--color-hairline); font-size: 0.75rem; }
  .range { color: var(--color-fg-2); font-weight: 500; }
  .kinds { display: flex; gap: 2px; padding: 2px; border: 1px solid var(--color-hairline); border-radius: 6px; background: var(--color-bg); }
  .kinds button { padding: 0.1rem 0.5rem; border-radius: 4px; font-size: 0.6875rem; color: var(--color-fg-3); cursor: pointer; }
  .kinds button.active { background: var(--color-question); color: var(--color-on-accent); font-weight: 500; }
  .kinds button.comment.active { background: var(--color-comment); }
  .scope { margin-left: auto; font-size: 0.625rem; color: var(--color-muted); font-family: "JetBrains Mono", monospace; }
  .close { color: var(--color-muted); cursor: pointer; font-size: 1rem; line-height: 1; }
  .quote { margin: 0.5rem 0.75rem 0; padding-left: 0.6rem; border-left: 2px solid var(--color-question); color: var(--color-fg-3); white-space: pre-wrap; max-height: 4.8em; overflow: hidden; }
  textarea { display: block; width: 100%; padding: 0.5rem 0.75rem; background: transparent; outline: none; resize: none; line-height: 1.5; color: var(--color-fg); }
  textarea::placeholder { color: var(--color-muted); }
  .footer { display: flex; align-items: center; justify-content: space-between; padding: 0.35rem 0.75rem; border-top: 1px solid var(--color-hairline); }
  .hint { font-size: 0.625rem; color: var(--color-muted); }
  .submit { padding: 0.2rem 0.7rem; border-radius: 4px; background: var(--color-action); color: var(--color-on-accent); font-size: 0.75rem; cursor: pointer; }
  .submit:disabled { opacity: 0.4; cursor: default; }
</style>
