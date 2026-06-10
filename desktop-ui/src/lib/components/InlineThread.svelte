<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { ThreadSnapshot } from "$lib/types";
  import PromoteModal from "$lib/components/PromoteModal.svelte";
  import EditMessageModal from "$lib/components/EditMessageModal.svelte";
  import ReplyActionBar from "$lib/components/ReplyActionBar.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { navigateToThread } from "$lib/dom";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";

  interface Props {
    thread: ThreadSnapshot;
    hunk_idx: number;
    variant?: "inline" | "panel";
  }

  const { thread, variant = "inline" }: Props = $props();

  const isQuestion = $derived(thread.kind === "question");
  const isPromoted = $derived(thread.promoted_to != null);

  let replyText = $state("");
  let showReply = $state(false);
  let replyTextarea: HTMLTextAreaElement | null = $state(null);
  let showPromote = $state(false);
  let editMessageId = $state<string | null>(null);
  let editInitialBody = $state("");

  let askAiText = $state("");
  let showAskAi = $state(false);
  let askAiTextarea: HTMLTextAreaElement | null = $state(null);

  let justCopied = $state(false);
  let pushing = $state(false);

  // Auto-focus the textarea when a composer opens. Opening one closes the other.
  $effect(() => {
    if (showReply && replyTextarea) {
      queueMicrotask(() => replyTextarea?.focus());
    }
  });
  $effect(() => {
    if (showAskAi && askAiTextarea) {
      queueMicrotask(() => askAiTextarea?.focus());
    }
  });

  function openReply() {
    showAskAi = false;
    askAiText = "";
    showReply = true;
  }
  function openAskAi() {
    showReply = false;
    replyText = "";
    showAskAi = true;
  }

  function formatTimestamp(ts: string): string {
    try {
      const d = new Date(ts);
      const now = Date.now();
      const diff = now - d.getTime();
      const mins = Math.floor(diff / 60000);
      const hours = Math.floor(diff / 3600000);
      const days = Math.floor(diff / 86400000);
      if (mins < 60) return `${Math.max(mins, 0)}m`;
      if (hours < 24) return `${hours}h`;
      return `${days}d`;
    } catch {
      return ts;
    }
  }

  function authorInitial(author: string): string {
    return (author || "?")[0].toUpperCase();
  }

  function avatarClass(kind: "you" | "human" | "ai"): string {
    if (kind === "ai") return "bg-ai/20";
    if (kind === "you") return "bg-accent text-black";
    return "bg-add-fg text-black";
  }

  async function deleteThread() {
    if (thread.replies.length > 0) {
      const n = thread.replies.length;
      const ok = confirm(
        `Delete this thread and its ${n} ${n === 1 ? "reply" : "replies"}? This can't be undone.`,
      );
      if (!ok) return;
    }
    await app.cmd("delete_thread", { id: thread.id });
  }

  async function deleteReply(replyId: string) {
    await app.cmd("delete_thread", { id: replyId });
  }

  async function submitReply() {
    if (!replyText.trim()) return;
    await app.cmd("reply_to_thread", { parentId: thread.id, text: replyText.trim() });
    replyText = "";
    showReply = false;
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

  async function submitPromote(body: string) {
    await app.cmd("promote_to_comment", { id: thread.id, body });
    showPromote = false;
  }

  async function submitAskAi() {
    const prompt = askAiText.trim();
    showAskAi = false;
    askAiText = "";
    await app.cmd("ask_ai", { threadId: thread.id, prompt });
  }

  async function validateWithAi() {
    await app.cmd("validate_with_ai", { threadId: thread.id, findingId: null });
  }

  async function copyThread() {
    const header = thread.line > 0 ? `${thread.file}:${thread.line}` : thread.file;
    const text = `**${header}**\n\n${buildPromoteBody()}`;
    await writeText(text);
    justCopied = true;
    setTimeout(() => (justCopied = false), 1500);
  }

  async function pushOnlyThis() {
    if (pushing || thread.synced || isQuestion) return;
    pushing = true;
    try {
      const activeTab = app.snapshot?.tabs?.find((t) => t.is_active) ?? null;
      const currentWorktree = app.snapshot?.worktrees.find((w) => w.is_current) ?? null;
      const prNumber =
        activeTab?.pr_number ?? currentWorktree?.pr_number ?? app.snapshot?.github?.number ?? app.snapshot?.pr?.number ?? null;
      await app.cmd("push_github_comment_thread", { id: thread.id, prNumber });
    } finally {
      pushing = false;
    }
  }

  async function resolveThread() {
    if (thread.resolved) return;
    await app.cmd("resolve_thread", { id: thread.id });
  }

  function openEdit(messageId: string, body: string) {
    editMessageId = messageId;
    editInitialBody = body;
  }

  async function submitEdit(body: string) {
    if (!editMessageId) return;
    await app.cmd("update_thread_message", { id: editMessageId, body });
    editMessageId = null;
  }

  const targetLineLabel = $derived(
    thread.line > 0 ? `${thread.file}:${thread.line}` : thread.file,
  );
</script>

<div
  id={thread.id}
  class="{variant === 'panel' ? '' : 'mx-4 my-3'} rounded-lg overflow-hidden font-sans border scroll-mt-16 min-w-0 max-w-full {thread.stale ? 'opacity-60' : ''} {isQuestion ? 'bg-question-surface border-question-border' : 'bg-card border-border'}"
>
  <!-- Header -->
  <div class="px-3 py-2 border-b border-hairline flex items-center gap-2">
    {#if isQuestion}
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-question"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>
      <span class="text-question text-sm font-medium">Local question</span>
    {:else}
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-comment"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
      <span class="text-fg-2 text-sm font-medium">Comment</span>
    {/if}
    {#if thread.line > 0}
      <span class="text-[10px] font-mono text-muted">· line {thread.line}</span>
    {/if}

    {#if isQuestion}
      <span class="ml-auto text-[10px] font-mono text-muted">private · won't push</span>
    {:else if !thread.synced && thread.source === "local"}
      <span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-full bg-hover border border-border font-mono text-[10px] text-ai ml-auto">
        <span class="w-1 h-1 rounded-full bg-ai"></span>local · unpushed
      </span>
    {:else if thread.synced}
      <span class="ml-auto text-add-fg text-[10px] font-mono">✓ synced</span>
    {:else}
      <div class="flex-1"></div>
    {/if}
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
    <span class="text-muted text-[10px] font-mono shrink-0">{formatTimestamp(thread.root.timestamp)}</span>
  </div>

  <!-- Root message -->
  <div class="px-3 py-2.5 flex gap-2.5 group/row">
    <div class="w-6 h-6 rounded-full flex items-center justify-center shrink-0 text-[11px] font-bold {avatarClass(thread.root.kind)}">
      {#if thread.root.kind === "ai"}
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ai"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
      {:else}
        {authorInitial(thread.root.author)}
      {/if}
    </div>
    <div class="flex-1 min-w-0">
      <div class="text-[11px] font-mono text-muted mb-0.5">
        {thread.root.kind === "you" ? "you" : thread.root.author} · {formatTimestamp(thread.root.timestamp)}
      </div>
      <div class="annotation-body-scroll">
        <MarkdownText text={thread.root.body_markdown} className="text-sm text-fg-2" />
      </div>
    </div>
    <div class="self-start shrink-0 flex gap-0.5 opacity-0 group-hover/row:opacity-60">
      {#if thread.root.kind === "you"}
        <button
          type="button"
          onclick={() => openEdit(thread.root.id, thread.root.body_markdown)}
          title="Edit message"
          aria-label="Edit message"
          class="p-0.5 rounded text-muted hover:!opacity-100 hover:text-fg-2 hover:bg-hover transition"
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.12 2.12 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
        </button>
      {/if}
      <button
        type="button"
        onclick={deleteThread}
        title={thread.replies.length > 0 ? "Delete thread (root + all replies)" : "Delete this thread"}
        aria-label="Delete thread"
        class="p-0.5 rounded text-muted hover:!opacity-100 hover:text-del-fg hover:bg-hover transition"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>
      </button>
    </div>
  </div>

  <!-- Replies -->
  {#if thread.replies.length > 0}
    <div class="border-t border-hairline bg-surface">
      {#each thread.replies as reply, i}
        <div class="px-3 py-2.5 flex gap-2.5 group/row {i > 0 ? 'border-t border-hairline' : ''}">
          <div class="w-6 h-6 rounded-full flex items-center justify-center shrink-0 text-[11px] font-bold {avatarClass(reply.kind)}">
            {#if reply.kind === "ai"}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ai"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
            {:else}
              {authorInitial(reply.author)}
            {/if}
          </div>
          <div class="flex-1 min-w-0 {reply.kind === 'ai' ? 'border-l-2 border-ai pl-2.5' : ''}">
            <div class="text-[11px] font-mono text-muted mb-0.5 flex items-center gap-1.5">
              {#if reply.kind === "ai"}<span class="text-ai font-medium font-sans">AI</span>{:else}<span>{reply.author}</span>{/if}
              {#if reply.timestamp}<span>· {formatTimestamp(reply.timestamp)}</span>{/if}
            </div>
            {#if reply.kind === "ai" && reply.body_markdown === "…thinking"}
              <div class="text-sm text-fg-3 italic animate-pulse">…thinking</div>
            {:else}
              <div class="annotation-body-scroll">
                <MarkdownText text={reply.body_markdown} className="text-sm text-fg-2" />
              </div>
            {/if}
          </div>
          {#if reply.id && reply.body_markdown !== "…thinking"}
            <ReplyActionBar
              reply={{ ...reply, origin: reply.origin ?? "thread_reply" }}
              rootThreadId={thread.id}
              {isQuestion}
              parentSynced={thread.synced}
              threadResolved={thread.resolved}
              onEdit={reply.kind === "you" ? () => openEdit(reply.id, reply.body_markdown) : undefined}
              onDelete={() => deleteReply(reply.id)}
            />
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Ask AI composer -->
  {#if showAskAi}
    <div class="px-3 py-2 border-t border-hairline">
      <textarea
        bind:this={askAiTextarea}
        bind:value={askAiText}
        placeholder="Add context for the AI… (leave empty for default · ⌘+Enter to send)"
        rows="3"
        class="w-full rounded-md border border-border bg-bg px-2 py-1.5 text-[13px] text-fg-2 placeholder:text-muted outline-none focus:border-ai resize-y font-mono"
        onkeydown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            submitAskAi();
          } else if (e.key === "Escape") {
            e.preventDefault();
            showAskAi = false;
            askAiText = "";
          }
        }}
      ></textarea>
      <div class="mt-1 flex items-center gap-2">
        <span class="text-[10px] font-mono text-muted">⌘+Enter to send · Esc to cancel · empty = default prompt</span>
        <button
          onclick={() => { showAskAi = false; askAiText = ""; }}
          class="ml-auto px-2 py-1 rounded-md text-[11px] text-fg-3 hover:bg-hover"
        >Cancel</button>
        <button onclick={submitAskAi} class="px-2 py-1 rounded-md text-[11px] text-ai hover:bg-hover border border-border">
          Ask AI
        </button>
      </div>
    </div>
  {/if}

  <!-- Inline reply composer -->
  {#if showReply}
    <div class="px-3 py-2 border-t border-hairline">
      <textarea
        bind:this={replyTextarea}
        bind:value={replyText}
        placeholder={isQuestion ? "Follow-up… (⌘+Enter to send)" : "Reply… (⌘+Enter to send)"}
        rows="3"
        class="w-full rounded-md border border-border bg-bg px-2 py-1.5 text-[13px] text-fg-2 placeholder:text-muted outline-none focus:border-accent resize-y font-mono"
        onkeydown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
            e.preventDefault();
            submitReply();
          } else if (e.key === "Escape") {
            e.preventDefault();
            showReply = false;
            replyText = "";
          }
        }}
      ></textarea>
      <div class="mt-1 flex items-center gap-2">
        <span class="text-[10px] font-mono text-muted">⌘+Enter to send · Esc to cancel</span>
        <button
          onclick={() => { showReply = false; replyText = ""; }}
          class="ml-auto px-2 py-1 rounded-md text-[11px] text-fg-3 hover:bg-hover"
        >Cancel</button>
        <button onclick={submitReply} disabled={!replyText.trim()} class="px-2 py-1 rounded-md text-[11px] text-fg-2 hover:bg-hover disabled:opacity-40 border border-border">
          Reply
        </button>
      </div>
    </div>
  {/if}

  <!-- Footer actions -->
  <div class="px-3 py-1.5 border-t border-hairline flex items-center gap-1 flex-wrap text-[11px]">
    {#if !showReply}
      <button onclick={openReply} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Reply</button>
    {/if}
    {#if !showAskAi}
      <button onclick={openAskAi} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Ask AI…</button>
    {/if}
    <button
      type="button"
      onclick={() => void validateWithAi()}
      title="Check this note against the current code (local reply, not posted to GitHub)"
      class="px-2 py-0.5 rounded text-ai hover:bg-hover flex items-center gap-1"
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>
      Validate with AI
    </button>
    <button
      onclick={copyThread}
      title="Copy thread as markdown"
      class="px-2 py-0.5 rounded hover:bg-hover flex items-center gap-1 {justCopied ? 'text-add-fg' : 'text-fg-3'}"
    >
      {#if justCopied}
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="20 6 9 17 4 12"/></svg>
        Copied
      {:else}
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        Copy
      {/if}
    </button>
    {#if !thread.resolved}
      <button onclick={resolveThread} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Resolve</button>
    {/if}
    {#if !isQuestion && !thread.synced}
      <button
        type="button"
        onclick={() => void pushOnlyThis()}
        disabled={pushing}
        title="Push this thread to GitHub (not a full review)"
        class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover flex items-center gap-1 disabled:opacity-40"
      >
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></svg>
        {pushing ? "Pushing…" : "Push only this"}
      </button>
    {/if}
    {#if isQuestion && !isPromoted}
      <button
        onclick={() => (showPromote = true)}
        class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover flex items-center gap-1"
      >
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        Promote to comment
      </button>
    {/if}
    <button
      type="button"
      onclick={deleteThread}
      title={thread.replies.length > 0 ? "Delete thread (root + all replies)" : "Delete this thread"}
      class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-del-fg"
    >Delete</button>
    {#if thread.replies.length > 0}
      <span class="ml-auto text-muted text-[10px]">{thread.replies.length} {thread.replies.length === 1 ? "reply" : "replies"}</span>
    {/if}
  </div>

  {#if isPromoted}
    <div class="px-3 py-1.5 border-t border-hairline text-[11px] font-mono text-muted flex items-center gap-1">
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
      <span>Promoted to <span class="text-fg-3">#{thread.promoted_to}</span></span>
    </div>
  {/if}
</div>

<PromoteModal
  open={showPromote}
  kind="question"
  sourceId={thread.id}
  initialBody={buildPromoteBody()}
  targetLineLabel={targetLineLabel}
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
