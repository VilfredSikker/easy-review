<script lang="ts">
  import { annotationDrafts } from "$lib/stores/annotationDrafts.svelte";
  import { app } from "$lib/stores/app.svelte";
  import type { ThreadSnapshot } from "$lib/types";
  import PromoteModal from "$lib/components/PromoteModal.svelte";
  import EditMessageModal from "$lib/components/EditMessageModal.svelte";
  import ReplyActionBar from "$lib/components/ReplyActionBar.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { navigateToThread } from "$lib/dom";
  import { copyToClipboard } from "$lib/clipboard";
  import AnnotationOverflowMenu from "$lib/components/AnnotationOverflowMenu.svelte";
  import { authorLabel, kindStyle, lineRangeLabel, threadActions, type OverflowItem } from "$lib/annotationActions";
  import { longAge } from "$lib/relativeTime";
  import { resolveActivePrNumber } from "$lib/prUrl";
  import { isDocumentAnchored, ownReplyCount } from "$lib/threadAnchor";

  interface Props {
    thread: ThreadSnapshot;
    hunk_idx: number;
    variant?: "inline" | "panel";
    /**
     * Anchored on a document line outside the diff (ADR 0041), which hides
     * Promote. Worked out from the snapshot when left unset.
     */
    documentAnchored?: boolean;
  }

  const { thread, variant = "inline", documentAnchored }: Props = $props();
  const draft = $derived(annotationDrafts.get(app.snapshot, "thread", thread.id));

  const isQuestion = $derived(thread.kind === "question");
  const isNote = $derived(thread.kind === "note");
  // Questions and notes are private, local-only, yellow-accented.
  const isLocal = $derived(isQuestion || isNote);
  const isPromoted = $derived(thread.promoted_to != null);

  let replyInput: HTMLInputElement | null = $state(null);
  let replyFocusPending = $state(false);
  let showPromote = $state(false);
  let editMessageId = $state<string | null>(null);
  let editInitialBody = $state("");

  let askAiTextarea: HTMLTextAreaElement | null = $state(null);
  let askAiFocusPending = $state(false);

  let justCopied = $state(false);
  let pushing = $state(false);

  // Auto-focus the textarea when a composer opens. Opening one closes the other.
  $effect(() => {
    if (replyFocusPending && draft.showReply && replyInput) {
      replyFocusPending = false;
      queueMicrotask(() => replyInput?.focus());
    }
  });
  $effect(() => {
    if (askAiFocusPending && draft.showAskAi && askAiTextarea) {
      askAiFocusPending = false;
      queueMicrotask(() => askAiTextarea?.focus());
    }
  });

  function openReply() {
    draft.showAskAi = false;
    draft.askAiText = "";
    draft.showReply = true;
    replyFocusPending = true;
  }
  function openAskAi() {
    draft.showReply = false;
    draft.replyText = "";
    draft.showAskAi = true;
    askAiFocusPending = true;
  }

  const formatTimestamp = (ts: string) => longAge(ts) || ts;

  function authorInitial(author: string): string {
    return (author || "?")[0].toUpperCase();
  }

  function avatarClass(kind: "you" | "human" | "ai"): string {
    if (kind === "ai") return "bg-ai/20";
    if (kind === "you") return "bg-accent text-on-accent";
    return "bg-add-fg text-on-accent";
  }

  // Native confirm() is a no-op in the Tauri webview (returns false, shows no
  // dialog), which silently aborted deletes of threads with replies. Use an
  // inline two-step confirm instead; the backend cascade-deletes root + replies.
  let confirmingDelete = $state(false);
  const deleteTitle = $derived.by(() => {
    if (confirmingDelete) return "Click again to confirm — deletes root + all replies";
    return thread.replies.length > 0 ? "Delete thread (root + all replies)" : "Delete this thread";
  });
  let confirmDeleteTimer: ReturnType<typeof setTimeout> | undefined;

  function deleteThread() {
    // Guard before touching the confirm state: a blocked paint must not eat
    // the user's confirm click.
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked("delete_thread");
    if (thread.replies.length > 0 && !confirmingDelete) {
      confirmingDelete = true;
      clearTimeout(confirmDeleteTimer);
      confirmDeleteTimer = setTimeout(() => (confirmingDelete = false), 3000);
      return;
    }
    clearTimeout(confirmDeleteTimer);
    confirmingDelete = false;
    void app.cmd("delete_thread", { id: thread.id });
  }

  function deleteReply(replyId: string) {
    void app.cmd("delete_thread", { id: replyId });
  }

  function submitReply() {
    const text = draft.replyText.trim();
    if (!text) return;
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    void app.cmd("reply_to_thread", { parentId: thread.id, text });
    draft.replyText = "";
    draft.showReply = false;
  }

  // Post-then-ask is one gesture: the AI answers the thread as it stands after
  // the reply lands, so the two commands must run in order. reply_to_thread is
  // optimistic and rolls back on failure without throwing, so whether the
  // reply landed is read off the snapshot once the write settles.
  async function replyAndAskAi() {
    const text = draft.replyText.trim();
    if (!text) return;
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    const before = ownReplyCount(app.snapshot, thread.id, text);
    const posted = app.cmd("reply_to_thread", { parentId: thread.id, text });
    draft.replyText = "";
    draft.showReply = false;
    await posted;
    if (ownReplyCount(app.snapshot, thread.id, text) <= before) {
      // Give the text back for a retry unless the reviewer has started anew.
      if (!draft.replyText) {
        draft.replyText = text;
        draft.showReply = true;
      }
      return;
    }
    await app.cmd("ask_ai", { threadId: thread.id, prompt: "" });
  }

  function buildPromoteBody(): string {
    const parts = [thread.root.body_markdown.trim()];
    for (const r of thread.replies) {
      const quoted = r.body_markdown
        .split("\n")
        .map((l) => `> ${l}`)
        .join("\n");
      parts.push(`> **${r.author}** replied:\n${quoted}`);
    }
    return parts.join("\n\n");
  }

  function submitPromote(body: string) {
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    void app.cmd("promote_to_comment", { id: thread.id, body });
    showPromote = false;
  }

  function promoteToNote() {
    void app.cmd("promote_to_note", { id: thread.id, body: buildPromoteBody() });
  }

  async function submitAskAi() {
    const prompt = draft.askAiText.trim();
    draft.showAskAi = false;
    draft.askAiText = "";
    await app.cmd("ask_ai", { threadId: thread.id, prompt });
  }

  async function validateWithAi() {
    await app.cmd("validate_with_ai", { threadId: thread.id, findingId: null });
  }

  async function elaborateWithAi() {
    await app.cmd("elaborate_with_ai", { threadId: thread.id });
  }

  async function copyThread() {
    const header = thread.line > 0 ? `${thread.file}:${thread.line}` : thread.file;
    const text = `**${header}**\n\n${buildPromoteBody()}`;
    await copyToClipboard(text);
    justCopied = true;
    setTimeout(() => (justCopied = false), 1500);
  }

  async function pushOnlyThis() {
    if (pushing || thread.synced || isLocal) return;
    pushing = true;
    try {
      const prNumber = resolveActivePrNumber(app.snapshot);
      await app.cmd("push_github_comment_thread", { id: thread.id, prNumber });
    } finally {
      pushing = false;
    }
  }

  function resolveThread() {
    if (thread.resolved) return;
    void app.cmd("resolve_thread", { id: thread.id });
  }

  function openEdit(messageId: string, body: string) {
    editMessageId = messageId;
    editInitialBody = body;
  }

  function submitEdit(body: string) {
    if (!editMessageId) return;
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    const id = editMessageId;
    void app.cmd("update_thread_message", { id, body });
    editMessageId = null;
  }

  const targetLineLabel = $derived(
    thread.line > 0 ? `${thread.file}:${thread.line}` : thread.file,
  );

  const actions = $derived(
    threadActions({
      kind: thread.kind,
      synced: thread.synced,
      resolved: thread.resolved,
      promoted: isPromoted,
      documentAnchored: documentAnchored ?? isDocumentAnchored(thread, app.snapshot?.files),
      rootEditable: thread.root.kind === "you",
    }),
  );

  const overflowItems = $derived.by((): OverflowItem[] => {
    const byId: Record<string, OverflowItem> = {
      copy: { id: "copy", label: justCopied ? "Copied" : "Copy thread", title: "Copy thread as markdown", onselect: () => void copyThread() },
      push: {
        id: "push",
        label: pushing ? "Pushing…" : "Push only this",
        title: "Push this thread to GitHub (not a full review)",
        disabled: pushing,
        onselect: () => void pushOnlyThis(),
      },
      promoteNote: { id: "promoteNote", label: "Promote to note", title: "Turn this question into a local actionable note", onselect: () => void promoteToNote() },
      resolve: { id: "resolve", label: "Resolve", onselect: resolveThread },
      edit: { id: "edit", label: "Edit", onselect: () => openEdit(thread.root.id, thread.root.body_markdown) },
      validate: { id: "validate", label: "Validate with AI", title: "Check this against the current code (local reply, not posted to GitHub)", onselect: () => void validateWithAi() },
      delete: { id: "delete", label: confirmingDelete ? "Confirm delete?" : "Delete", title: deleteTitle, danger: true, onselect: deleteThread },
    };
    return actions.overflow.map((id) => byId[id]);
  });

  const kindChip = $derived(kindStyle(thread.kind));
  const rangeLabel = $derived(lineRangeLabel(thread.line, thread.line_end));
</script>

{#snippet avatar(kind: "you" | "human" | "ai", author: string)}
  <span class="w-[18px] h-[18px] rounded-full inline-grid place-items-center shrink-0 text-[9px] font-semibold {avatarClass(kind)}">
    {#if kind === "ai"}
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ai"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
    {:else}
      {authorInitial(author)}
    {/if}
  </span>
{/snippet}

<div
  id={thread.id}
  style="--kc: {kindChip.color}; box-shadow: inset 3px 0 0 var(--kc);"
  class="{variant === 'panel' ? '' : 'my-3'} grid gap-1.5 rounded-[9px] px-3 py-2.5 font-sans border border-border bg-card scroll-mt-16 min-w-0 max-w-full {thread.stale || thread.resolved ? 'opacity-60' : ''}"
>
  <!-- Header -->
  <div class="flex items-center gap-2 flex-wrap text-xs">
    <span
      class="text-[10.5px] font-semibold px-1.5 leading-[17px] rounded"
      style="color: var(--kc); background: color-mix(in srgb, var(--kc) 14%, transparent);"
    >{kindChip.label}</span>
    {@render avatar(thread.root.kind, thread.root.author)}
    <b class="font-semibold text-fg-2">{authorLabel(thread.root.kind, thread.root.author)}</b>
    {#if rangeLabel}
      <span class="text-[11px] font-mono text-muted">{rangeLabel}</span>
    {/if}
    <span class="text-muted">{formatTimestamp(thread.root.timestamp)}</span>
    {#if thread.stale}
      <span class="text-[10px] font-mono px-1.5 py-0.5 rounded bg-hover text-fg-3">stale</span>
    {/if}
    {#if variant === "panel"}
      <button
        type="button"
        onclick={() => navigateToThread(thread)}
        title="Jump to inline location"
        aria-label="Jump to inline location"
        class="p-0.5 rounded text-muted hover:text-accent hover:bg-hover transition"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M7 17L17 7M7 7h10v10"/></svg>
      </button>
    {/if}
    <span class="ml-auto text-[11px] text-fg-3">
      {#if isLocal}
        only you
      {:else if thread.synced}
        GitHub · synced
      {:else if thread.source === "local"}
        <span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-hover border border-border font-mono text-[10px] text-ai"><span class="w-1 h-1 rounded-full bg-ai"></span>local · unpushed</span>
      {/if}
    </span>
  </div>

  <!-- Root message -->
  <div class="annotation-body-scroll">
    <MarkdownText text={thread.root.body_markdown} className="text-sm text-fg-2 leading-[1.55]" />
  </div>

  <!-- Replies -->
  {#each thread.replies as reply, i (i)}
    <div
      class="group/row grid grid-cols-[18px_minmax(0,1fr)] gap-2 pt-2 border-t border-hairline {reply.kind === 'ai' ? '-mx-3 px-3 pb-0.5' : ''}"
      style={reply.kind === "ai" ? "background: linear-gradient(90deg, color-mix(in srgb, var(--color-accent) 6%, transparent), transparent 60%);" : ""}
    >
      {@render avatar(reply.kind, reply.author)}
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-xs">
          <b class="font-semibold text-fg-2">{authorLabel(reply.kind, reply.author)}</b>
          {#if reply.kind === "ai"}<span class="inline-flex items-center h-4 px-1.5 rounded-full bg-hover text-[10px] text-fg-3">AI</span>{/if}
          {#if reply.timestamp}<span class="text-muted">{formatTimestamp(reply.timestamp)}</span>{/if}
        </div>
        {#if reply.kind === "ai" && reply.body_markdown === "…thinking"}
          <div class="text-sm text-fg-3 italic animate-pulse">Thinking…</div>
        {:else}
          <div class="annotation-body-scroll">
            <MarkdownText text={reply.body_markdown} className="text-sm {reply.kind === 'ai' ? 'text-fg-2' : 'text-fg'} leading-[1.55]" />
          </div>
        {/if}
        {#if reply.id && reply.body_markdown !== "…thinking"}
          <ReplyActionBar
            reply={{ ...reply, origin: reply.origin ?? "thread_reply" }}
            rootThreadId={thread.id}
            isQuestion={isLocal}
            parentSynced={thread.synced}
            threadResolved={thread.resolved}
            onEdit={reply.kind === "you" ? () => openEdit(reply.id, reply.body_markdown) : undefined}
            onDelete={() => deleteReply(reply.id)}
          />
        {/if}
      </div>
    </div>
  {/each}

  <!-- Ask AI composer (context for the prompt) -->
  {#if draft.showAskAi}
    <div class="pt-1">
      <textarea
        bind:this={askAiTextarea}
        bind:value={draft.askAiText}
        placeholder="Add context for the AI… (leave empty for default · ⌘+Enter to send)"
        rows="3"
        class="w-full rounded-md border border-border bg-bg px-2 py-1.5 text-[13px] text-fg-2 placeholder:text-muted outline-none focus:border-accent resize-y font-mono"
        onkeydown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            submitAskAi();
          } else if (e.key === "Escape") {
            e.preventDefault();
            draft.showAskAi = false;
            draft.askAiText = "";
          }
        }}
      ></textarea>
      <div class="mt-1 flex items-center gap-2">
        <span class="text-[10px] font-mono text-muted">⌘+Enter to send · Esc to cancel · empty = default prompt</span>
        <button type="button"
          onclick={() => { draft.showAskAi = false; draft.askAiText = ""; }}
          class="ml-auto px-2 py-1 rounded-md text-[11px] text-fg-3 hover:bg-hover"
        >Cancel</button>
        <button type="button" onclick={submitAskAi} class="px-2 py-1 rounded-md text-[11px] text-accent hover:bg-accent-soft border border-accent-border">
          ✦ Ask AI
        </button>
      </div>
    </div>
  {/if}

  <!-- Reply box: roots only (ADR 0008). -->
  {#if draft.showReply}
    <form
      class="flex gap-1.5"
      onsubmit={(e) => { e.preventDefault(); submitReply(); }}
    >
      <input
        bind:this={replyInput}
        bind:value={draft.replyText}
        placeholder={isQuestion ? "Follow-up…" : "Reply…"}
        aria-label="Reply"
        class="flex-1 min-w-0 h-[26px] px-2 rounded-md border border-border bg-bg text-[13px] text-fg-2 placeholder:text-muted outline-none focus:border-accent"
        onkeydown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            draft.showReply = false;
            draft.replyText = "";
          }
        }}
      />
      <button type="submit" disabled={!draft.replyText.trim()} class="h-[26px] px-2.5 rounded-md text-xs text-fg-2 border border-border hover:bg-hover disabled:opacity-40">Reply</button>
      <button
        type="button"
        onclick={() => void replyAndAskAi()}
        disabled={!draft.replyText.trim()}
        title="Post your reply, then have AI answer it locally"
        class="h-[26px] px-2.5 rounded-md text-xs text-accent border border-accent-border hover:bg-accent-soft disabled:opacity-40"
      >✦ Ask AI</button>
    </form>
  {/if}

  <!-- Actions -->
  <div class="flex items-center gap-0.5 flex-wrap -mx-1.5 text-xs">
    {#if thread.resolved}
      <span class="px-1.5 text-fg-3">Resolved</span>
    {/if}
    {#each actions.primary as action (action)}
      {#if action === "askAi"}
        {#if !draft.showAskAi}
          <button type="button" onclick={openAskAi} title="Ask AI about this" class="px-[7px] py-0.5 rounded-[5px] text-accent hover:bg-accent-soft">✦ Ask AI</button>
        {/if}
      {:else if action === "elaborate"}
        <button
          type="button"
          onclick={() => void elaborateWithAi()}
          title="Ask AI to expand this (local reply, not posted to GitHub)"
          class="px-[7px] py-0.5 rounded-[5px] text-accent hover:bg-accent-soft"
        >✦ Elaborate</button>
      {:else if action === "reply"}
        {#if !draft.showReply}
          <button type="button" onclick={openReply} class="px-[7px] py-0.5 rounded-[5px] text-fg-2 hover:bg-hover hover:text-fg">Reply</button>
        {/if}
      {:else if action === "resolve"}
        <button type="button" onclick={resolveThread} class="px-[7px] py-0.5 rounded-[5px] text-fg-2 hover:bg-hover hover:text-fg">Resolve</button>
      {:else if action === "promote"}
        <button
          type="button"
          onclick={() => (showPromote = true)}
          title="Promote to a GitHub comment"
          class="px-[7px] py-0.5 rounded-[5px] text-fg-2 hover:bg-hover hover:text-fg"
        >Promote</button>
      {:else if action === "delete"}
        <button
          type="button"
          onclick={deleteThread}
          title={deleteTitle}
          class="px-[7px] py-0.5 rounded-[5px] {confirmingDelete ? 'text-del-fg bg-hover font-medium' : 'text-fg-2 hover:bg-hover hover:text-del-fg'}"
        >{confirmingDelete ? "Confirm delete?" : "Delete"}</button>
      {/if}
    {/each}
    <AnnotationOverflowMenu items={overflowItems} />
    {#if thread.replies.length > 0}
      <span class="ml-auto pr-1.5 text-muted text-[10px]">{thread.replies.length} {thread.replies.length === 1 ? "reply" : "replies"}</span>
    {/if}
  </div>

  {#if isPromoted}
    <div class="text-[11px] font-mono text-muted flex items-center gap-1">
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
      <span>Promoted to <span class="text-fg-3">#{thread.promoted_to}</span></span>
    </div>
  {/if}
</div>

<PromoteModal
  open={showPromote}
  kind={isNote ? "note" : "question"}
  sourceId={thread.id}
  initialBody={buildPromoteBody()}
  {targetLineLabel}
  onSubmit={submitPromote}
  onClose={() => (showPromote = false)}
/>

<EditMessageModal
  open={editMessageId != null}
  messageId={editMessageId ?? ""}
  initialBody={editInitialBody}
  onSubmit={submitEdit}
  onClose={() => (editMessageId = null)}
/>
