<script lang="ts">
  import type { FlatFinding, ThreadSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import PromoteModal from "$lib/components/PromoteModal.svelte";
  import EditMessageModal from "$lib/components/EditMessageModal.svelte";
  import ReplyActionBar from "$lib/components/ReplyActionBar.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";

  type MergedReply = {
    id: string;
    author: string;
    kind: "you" | "human" | "ai";
    timestamp: string;
    body_markdown: string;
    origin: "finding_response" | "thread_reply";
    source?: string;
    synced?: boolean;
    editable?: boolean;
    deletable?: boolean;
  };

  interface Props {
    finding: FlatFinding;
    thread?: ThreadSnapshot | null;
  }

  const { finding, thread = null }: Props = $props();

  const severityColor = $derived(
    finding.severity === "high" ? "var(--color-risk-high)"
    : finding.severity === "med" ? "var(--color-risk-med)"
    : "var(--color-risk-low)",
  );

  const isPromoted = $derived(finding.promoted_to != null);

  const agentLabel = $derived(finding.agent_label ?? finding.expert_label ?? "General");
  const isProfessor = $derived(agentLabel === "Professor");
  const headerKind = $derived(isProfessor ? "Insight" : "Finding");

  const agentPillStyle = $derived(
    agentLabel === "Professor"
      ? "background: color-mix(in srgb, var(--color-emphasis) 15%, transparent); color: var(--color-emphasis); border-color: color-mix(in srgb, var(--color-emphasis) 25%, transparent)"
      : agentLabel === "General"
        ? "background: color-mix(in srgb, var(--color-fg-3) 15%, transparent); color: var(--color-fg-3); border-color: color-mix(in srgb, var(--color-fg-3) 25%, transparent)"
        : "background: color-mix(in srgb, var(--color-info) 15%, transparent); color: var(--color-info); border-color: color-mix(in srgb, var(--color-info) 25%, transparent)",
  );

  let replyText = $state("");
  let showPromote = $state(false);
  let editMessageId = $state<string | null>(null);
  let editOrigin = $state<"finding_response" | "thread_reply" | null>(null);
  let editInitialBody = $state("");
  let replyInputEl = $state<HTMLInputElement | null>(null);

  const mergedReplies = $derived.by((): MergedReply[] => {
    const byKey = new Map<string, MergedReply>();
    const add = (r: MergedReply) => {
      const key = `${r.origin}:${r.id || r.timestamp}:${r.body_markdown}`;
      if (!byKey.has(key)) byKey.set(key, r);
    };
    for (const r of finding.responses ?? []) {
      add({
        id: r.id,
        author: r.author,
        kind: r.kind,
        timestamp: r.timestamp,
        body_markdown: r.body_markdown,
        origin: "finding_response",
        editable: r.editable,
        deletable: r.deletable,
      });
    }
    for (const r of thread?.replies ?? []) {
      add({
        id: r.id,
        author: r.author,
        kind: r.kind,
        timestamp: r.timestamp,
        body_markdown: r.body_markdown,
        origin: r.origin ?? "thread_reply",
        source: r.source,
        synced: r.synced,
        editable: r.editable ?? r.kind === "you",
        deletable: r.deletable ?? true,
      });
    }
    return [...byKey.values()].sort((a, b) => a.timestamp.localeCompare(b.timestamp));
  });

  function focusReply() {
    replyInputEl?.focus();
  }

  async function dismiss() {
    await app.cmd("dismiss_finding", { findingId: finding.id });
  }
  async function reply() {
    if (!replyText.trim()) return;
    await app.cmd("reply_to_finding", { findingId: finding.id, body: replyText.trim(), aiAssist: false });
    replyText = "";
  }
  async function askAi() {
    if (thread) {
      await app.cmd("ask_ai", {
        threadId: thread.id,
        prompt: "Elaborate on this and answer any question directly.",
      });
    } else {
      await app.cmd("reply_to_finding", {
        findingId: finding.id,
        body: "Elaborate on this and answer any question directly.",
        aiAssist: true,
      });
    }
  }

  function openEdit(reply: MergedReply) {
    editMessageId = reply.id;
    editOrigin = reply.origin;
    editInitialBody = reply.body_markdown;
  }

  async function submitEdit(body: string) {
    if (!editMessageId || !editOrigin) return;
    if (editOrigin === "finding_response") {
      await app.cmd("update_finding_response", {
        findingId: finding.id,
        responseId: editMessageId,
        body,
      });
    } else {
      await app.cmd("update_thread_message", { id: editMessageId, body });
    }
    editMessageId = null;
    editOrigin = null;
  }

  async function validateWithAi() {
    await app.cmd("validate_with_ai", {
      threadId: thread?.id ?? null,
      findingId: finding.id,
    });
  }

  async function deleteReply(replyId: string, origin: MergedReply["origin"]) {
    if (!replyId) return;
    if (origin === "finding_response") {
      await app.cmd("delete_finding_response", {
        findingId: finding.id,
        responseId: replyId,
      });
    } else {
      await app.cmd("delete_thread", { id: replyId });
    }
  }

  async function deleteConversation() {
    await app.cmd("remove_finding_thread", { findingId: finding.id });
  }

  function buildPromoteBody(): string {
    const parts = [
      finding.message_markdown
        ? `${finding.title}\n\n${finding.message_markdown}`
        : finding.title,
    ];
    for (const r of mergedReplies) {
      if (r.body_markdown === "…thinking") continue;
      const quoted = r.body_markdown
        .split("\n")
        .map((l) => `> ${l}`)
        .join("\n");
      parts.push(`> **${r.author}** replied:\n${quoted}`);
    }
    return parts.join("\n\n");
  }

  async function submitPromote(body: string) {
    await app.cmd("promote_finding_to_comment", { findingId: finding.id, body });
    showPromote = false;
  }

  const targetLineLabel = $derived(
    finding.line != null ? `${finding.file}:${finding.line}` : finding.file,
  );

  function formatTimestamp(ts: string): string {
    try {
      const diff = Date.now() - new Date(ts).getTime();
      const mins = Math.floor(diff / 60000);
      const hours = Math.floor(diff / 3600000);
      if (mins < 60) return `${Math.max(mins, 0)}m`;
      if (hours < 24) return `${hours}h`;
      return `${Math.floor(diff / 86400000)}d`;
    } catch { return ts; }
  }

</script>

<div
  id="finding-{finding.id}"
  class="my-3 border rounded-lg overflow-hidden font-sans scroll-mt-16 min-w-0 max-w-full"
  style="border-color: color-mix(in srgb, {severityColor} 30%, transparent); background: color-mix(in srgb, {severityColor} 4%, transparent);"
>
  <!-- Header -->
  <div class="px-3 py-2 border-b border-hairline flex items-center gap-2 text-xs flex-wrap">
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="shrink-0" style="color: {severityColor}"><circle cx="12" cy="12" r="10"/><path d="M8 12l3 3 5-6"/></svg>
    <span class="font-medium shrink-0" style="color: {severityColor}">{headerKind}</span>
    <span
      class="px-1.5 py-0.5 rounded-full text-[9px] font-medium border shrink-0"
      style={agentPillStyle}
      title="Review agent"
    >{agentLabel}</span>
    {#if !isProfessor}
      <span class="px-1.5 py-0.5 rounded-full text-[9px] uppercase tracking-wider font-medium shrink-0" style="background: color-mix(in srgb, {severityColor} 15%, transparent); color: {severityColor}">
        {finding.severity}
      </span>
    {/if}
    {#if finding.line !== null}
      <span class="text-muted">· line {finding.line}</span>
    {/if}
    <span class="ml-auto text-[10px] mono text-muted">AI</span>
  </div>

  <!-- Body -->
  <div class="px-3 py-2 text-sm text-fg-2 min-w-0">
    <div class="annotation-body-scroll">
      <MarkdownText text={finding.title} className="text-sm text-fg-2" />
      {#if finding.message_markdown}
        <MarkdownText text={finding.message_markdown} className="text-fg-3 mt-1" />
      {/if}
    </div>
  </div>

  <!-- Actions on the finding (not on replies below) -->
  <div class="px-3 py-1.5 border-t border-hairline flex items-center gap-2 text-[11px] flex-wrap">
    {#if !isPromoted}
      <button onclick={() => (showPromote = true)} class="px-2 py-0.5 rounded text-comment hover:bg-hover flex items-center gap-1">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        Promote to comment
      </button>
    {/if}
    <button onclick={focusReply} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Reply</button>
    <button
      type="button"
      onclick={() => void askAi()}
      title="Ask AI to elaborate on this finding"
      class="px-2 py-0.5 rounded text-ai hover:bg-hover flex items-center gap-1"
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
      Ask AI
    </button>
    <button
      type="button"
      onclick={() => void validateWithAi()}
      title="Check this finding against the current code (local reply, not posted to GitHub)"
      class="px-2 py-0.5 rounded text-ai hover:bg-hover flex items-center gap-1"
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>
      Validate with AI
    </button>
    {#if thread}
      <button
        type="button"
        onclick={() => void deleteConversation()}
        title="Remove validation / AI replies on this finding"
        class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-del-fg"
      >Remove thread</button>
    {/if}
    <button type="button" onclick={dismiss} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-del-fg" title="Remove finding from review">Dismiss finding</button>
    <span class="ml-auto kbd">⇧R</span>
  </div>

  <!-- Validation / AI replies (finding.responses + legacy thread replies) -->
  {#if mergedReplies.length > 0}
    <div class="border-t border-hairline bg-surface">
      {#each mergedReplies as reply, i}
        <div class="px-3 py-2.5 flex gap-2.5 group/row {i > 0 ? 'border-t border-hairline' : ''}">
          <div class="w-6 h-6 rounded-full flex items-center justify-center shrink-0 text-[11px] font-bold {reply.kind === 'ai' ? 'bg-ai/20' : 'bg-accent text-on-accent'}">
            {#if reply.kind === "ai"}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ai"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
            {:else}
              {(reply.author || "Y")[0].toUpperCase()}
            {/if}
          </div>
          <div class="flex-1 min-w-0 {reply.kind === 'ai' ? 'border-l-2 border-ai pl-2.5' : ''}">
            <div class="text-[11px] font-mono text-muted mb-0.5">
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
            {#if reply.id && reply.body_markdown !== "…thinking"}
              <ReplyActionBar
                {reply}
                rootThreadId={thread?.id ?? null}
                findingId={finding.id}
                isQuestion={thread?.kind === "question"}
                parentSynced={thread?.synced ?? false}
                threadResolved={thread?.resolved ?? false}
                onEdit={reply.editable ? () => openEdit(reply) : undefined}
                onDelete={() => deleteReply(reply.id, reply.origin)}
              />
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Reply composer -->
  <div class="px-3 py-2 border-t border-hairline flex items-center gap-2">
    <input
      bind:this={replyInputEl}
      bind:value={replyText}
      onkeydown={(e) => {
        if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); reply(); }
        else if (e.key === "Escape") { replyText = ""; }
      }}
      placeholder="Reply to this finding…"
      class="bg-transparent flex-1 text-[13px] outline-none placeholder:text-muted"
    />
  </div>

  {#if isPromoted}
    <div class="px-3 py-1.5 border-t border-hairline text-[11px] font-mono text-muted flex items-center gap-1">
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
      <span>Promoted to <span class="text-fg-3">#{finding.promoted_to}</span></span>
    </div>
  {/if}
</div>

<PromoteModal
  open={showPromote}
  kind="finding"
  sourceId={finding.id}
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
