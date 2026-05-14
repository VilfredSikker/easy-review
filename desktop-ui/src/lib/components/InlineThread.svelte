<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import type { ThreadSnapshot } from "$lib/types";
  import PromoteModal from "$lib/components/PromoteModal.svelte";

  interface Props {
    thread: ThreadSnapshot;
    hunk_idx: number;
  }

  const { thread }: Props = $props();

  const isQuestion = $derived(thread.kind === "question");
  const isPromoted = $derived(thread.promoted_to != null);

  let replyText = $state("");
  let showReply = $state(false);
  let replyTextarea: HTMLTextAreaElement | null = $state(null);
  let showPromote = $state(false);

  // Auto-focus the textarea when the composer opens.
  $effect(() => {
    if (showReply && replyTextarea) {
      queueMicrotask(() => replyTextarea?.focus());
    }
  });

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
    await app.cmd("delete_thread", { id: thread.id });
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

  async function askAi() {
    await app.cmd("ask_ai", {
      threadId: thread.id,
      prompt: "Elaborate on this and answer any question directly.",
    });
  }

  async function resolveThread() {
    if (thread.resolved) return;
    await app.cmd("resolve_thread", { id: thread.id });
  }

  const targetLineLabel = $derived(
    thread.line > 0 ? `${thread.file}:${thread.line}` : thread.file,
  );
</script>

<div
  id={thread.id}
  class="mx-4 my-3 rounded-lg overflow-hidden font-sans border scroll-mt-16 {thread.stale ? 'opacity-60' : ''} {isQuestion ? 'bg-question-surface border-question-border' : 'bg-card border-border'}"
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
    <span class="text-muted text-[10px] font-mono shrink-0">{formatTimestamp(thread.root.timestamp)}</span>
  </div>

  <!-- Root message -->
  <div class="px-3 py-2.5 flex gap-2.5">
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
      <div class="text-sm text-fg-2 whitespace-pre-wrap">{thread.root.body_markdown}</div>
    </div>
  </div>

  <!-- Replies -->
  {#if thread.replies.length > 0}
    <div class="border-t border-hairline bg-surface">
      {#each thread.replies as reply, i}
        <div class="px-3 py-2.5 flex gap-2.5 {i > 0 ? 'border-t border-hairline' : ''}">
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
              <div class="text-sm text-fg-2 whitespace-pre-wrap">{reply.body_markdown}</div>
            {/if}
          </div>
        </div>
      {/each}
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
  <div class="px-3 py-1.5 border-t border-hairline flex items-center gap-1 flex-wrap">
    {#if !showReply}
      <button onclick={() => (showReply = true)} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Reply</button>
    {/if}
    <button
      onclick={askAi}
      class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover"
    >Ask AI</button>
    {#if !thread.resolved}
      <button onclick={resolveThread} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Resolve</button>
    {/if}
    {#if !isQuestion && !thread.synced}
      <button class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover flex items-center gap-1">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></svg>
        Push only this
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
    <button onclick={deleteThread} aria-label="Delete thread" class="ml-auto px-2 py-0.5 rounded text-muted hover:text-del-fg hover:bg-hover">×</button>
    {#if thread.replies.length > 0}
      <span class="text-muted text-[10px]">{thread.replies.length} {thread.replies.length === 1 ? "reply" : "replies"}</span>
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
