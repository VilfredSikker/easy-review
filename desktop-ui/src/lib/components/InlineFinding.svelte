<script lang="ts">
  import type { FlatFinding, ThreadSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import PromoteModal from "$lib/components/PromoteModal.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";

  interface Props {
    finding: FlatFinding;
    thread?: ThreadSnapshot | null;
  }

  const { finding, thread = null }: Props = $props();

  const severityColor = $derived(
    finding.severity === "high" ? "#ef4444"
    : finding.severity === "med" ? "#fbbf24"
    : "#60a5fa",
  );

  const isPromoted = $derived(finding.promoted_to != null);

  const agentLabel = $derived(finding.agent_label ?? finding.expert_label ?? "General");
  const isProfessor = $derived(agentLabel === "Professor");
  const headerKind = $derived(isProfessor ? "Insight" : "Finding");

  const agentPillStyle = $derived(
    agentLabel === "Professor"
      ? "background: #f9731626; color: #fb923c; border-color: #f9731640"
      : agentLabel === "General"
        ? "background: #94a3b826; color: #94a3b8; border-color: #94a3b840"
        : "background: #38bdf826; color: #38bdf8; border-color: #38bdf840",
  );

  let replyText = $state("");
  let showPromote = $state(false);
  let replyInputEl = $state<HTMLInputElement | null>(null);

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
    replyText = "";
  }

  async function validateWithAi() {
    if (thread) {
      await app.cmd("validate_with_ai", { threadId: thread.id, findingId: null });
    } else {
      await app.cmd("validate_with_ai", { threadId: null, findingId: finding.id });
    }
  }

  function buildPromoteBody(): string {
    return finding.message_markdown
      ? `${finding.title}\n\n${finding.message_markdown}`
      : finding.title;
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

  async function askAiOnThread() {
    if (!thread) return;
    await app.cmd("ask_ai", {
      threadId: thread.id,
      prompt: "Elaborate on this and answer any question directly.",
    });
  }
</script>

<div
  id="finding-{finding.id}"
  class="mx-4 my-3 border rounded-lg overflow-hidden font-sans scroll-mt-16 min-w-0 max-w-full"
  style="border-color: {severityColor}4d; background: {severityColor}0a;"
>
  <!-- Header -->
  <div class="px-3 py-2 border-b border-hairline flex items-center gap-2 text-xs flex-wrap">
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke={severityColor} stroke-width="2.5" class="shrink-0"><circle cx="12" cy="12" r="10"/><path d="M8 12l3 3 5-6"/></svg>
    <span class="font-medium shrink-0" style="color: {severityColor}">{headerKind}</span>
    <span
      class="px-1.5 py-0.5 rounded-full text-[9px] font-medium border shrink-0"
      style={agentPillStyle}
      title="Review agent"
    >{agentLabel}</span>
    {#if !isProfessor}
      <span class="px-1.5 py-0.5 rounded-full text-[9px] uppercase tracking-wider font-medium shrink-0" style="background: {severityColor}26; color: {severityColor}">
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

  <!-- Inline AI thread replies (created via Ask AI) -->
  {#if thread}
    <div class="border-t border-hairline bg-surface">
      <!-- Root message (the "AI follow-up requested" stub) is hidden; show replies only -->
      {#if thread.replies.length === 0}
        <div class="px-3 py-2.5 flex gap-2.5 items-center text-[12px] text-muted italic animate-pulse">
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ai shrink-0"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
          AI is thinking…
        </div>
      {:else}
        {#each thread.replies as reply, i}
          <div class="px-3 py-2.5 flex gap-2.5 {i > 0 ? 'border-t border-hairline' : ''}">
            <div class="w-6 h-6 rounded-full flex items-center justify-center shrink-0 text-[11px] font-bold {reply.kind === 'ai' ? 'bg-ai/20' : 'bg-accent text-black'}">
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
            </div>
          </div>
        {/each}
      {/if}
    </div>
  {/if}

  <!-- Action footer -->
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
      onclick={() => void validateWithAi()}
      title="Check this finding against the current code (local reply, not posted to GitHub)"
      class="px-2 py-0.5 rounded text-ai hover:bg-hover flex items-center gap-1"
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M9 12l2 2 4-4"/><circle cx="12" cy="12" r="10"/></svg>
      Validate with AI
    </button>
    <button type="button" onclick={dismiss} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Dismiss</button>
    <span class="ml-auto kbd">⇧R</span>
  </div>

  <!-- Reply composer -->
  <div class="px-3 py-2 border-t border-hairline flex items-center gap-2">
    <input
      bind:this={replyInputEl}
      bind:value={replyText}
      onkeydown={(e) => {
        if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); reply(); }
        else if (e.key === "Escape") { replyText = ""; }
      }}
      placeholder="Reply or ask follow-up…"
      class="bg-transparent flex-1 text-[13px] outline-none placeholder:text-muted"
    />
    <button
      onclick={askAi}
      class="flex items-center gap-1 px-2 py-1 rounded-md text-[11px] hover:bg-hover font-medium"
      style="color: {severityColor}"
    >
      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
      Ask AI
    </button>
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
