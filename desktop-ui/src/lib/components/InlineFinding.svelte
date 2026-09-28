<script lang="ts">
  import type { FlatFinding, ThreadSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import PromoteModal from "$lib/components/PromoteModal.svelte";
  import EditMessageModal from "$lib/components/EditMessageModal.svelte";
  import ReplyActionBar from "$lib/components/ReplyActionBar.svelte";
  import MarkdownText from "$lib/components/ui/MarkdownText.svelte";
  import { confidenceGlyph } from "$lib/diffAnnotations";
  import { mergeFindingReplies, type MergedReply } from "$lib/findingReplies";

  interface Props {
    finding: FlatFinding;
    thread?: ThreadSnapshot | null;
  }

  const { finding, thread = null }: Props = $props();

  function severityColorFor(severity: FlatFinding["severity"]): string {
    if (severity === "high") return "var(--color-risk-high)";
    if (severity === "med") return "var(--color-risk-med)";
    return "var(--color-risk-low)";
  }
  const severityColor = $derived(severityColorFor(finding.severity));

  const isPromoted = $derived(finding.promoted_to != null);

  const agentLabel = $derived(finding.agent_label ?? finding.expert_label ?? "General");
  const isProfessor = $derived(agentLabel === "Professor");
  const headerKind = $derived(isProfessor ? "Insight" : "Finding");

  /// The engine's `producers · category` tag, hidden when it only repeats the
  /// agent pill beside it.
  const lensCategory = $derived(finding.lens_category.trim());
  const showLensCategory = $derived(
    lensCategory.length > 0 && lensCategory.toLowerCase() !== agentLabel.toLowerCase(),
  );

  function agentPillColor(label: string): string {
    if (label === "Professor") return "var(--color-emphasis)";
    if (label === "General") return "var(--color-fg-3)";
    return "var(--color-info)";
  }
  const agentPillStyle = $derived.by(() => {
    const color = agentPillColor(agentLabel);
    return `background: color-mix(in srgb, ${color} 15%, transparent); color: ${color}; border-color: color-mix(in srgb, ${color} 25%, transparent)`;
  });

  let replyText = $state("");
  let showPromote = $state(false);
  let editMessageId = $state<string | null>(null);
  let editOrigin = $state<"finding_response" | "thread_reply" | null>(null);
  let editInitialBody = $state("");
  let replyInputEl = $state<HTMLInputElement | null>(null);

  const mergedReplies = $derived(mergeFindingReplies(finding, thread));

  function focusReply() {
    replyInputEl?.focus();
  }

  function dismiss() {
    void app.cmd("dismiss_finding", { findingId: finding.id });
  }
  function reply() {
    const body = replyText.trim();
    if (!body) return;
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked("reply_to_finding");
    void app.cmd("reply_to_finding", { findingId: finding.id, body, aiAssist: false });
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

  function openEdit(target: MergedReply) {
    editMessageId = target.id;
    editOrigin = target.origin;
    editInitialBody = target.body_markdown;
  }

  function submitEdit(body: string) {
    if (!editMessageId || !editOrigin) return;
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked();
    const id = editMessageId;
    const origin = editOrigin;
    if (origin === "finding_response") {
      void app.cmd("update_finding_response", {
        findingId: finding.id,
        responseId: id,
        body,
      });
    } else {
      void app.cmd("update_thread_message", { id, body });
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

  function deleteReply(replyId: string, origin: MergedReply["origin"]) {
    if (!replyId) return;
    if (origin === "finding_response") {
      void app.cmd("delete_finding_response", {
        findingId: finding.id,
        responseId: replyId,
      });
    } else {
      void app.cmd("delete_thread", { id: replyId });
    }
  }

  function deleteConversation() {
    void app.cmd("remove_finding_thread", { findingId: finding.id });
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

  function submitPromote(body: string) {
    if (!app.canPaintOptimistic()) return app.explainPaintBlocked("promote_finding_to_comment");
    void app.cmd("promote_finding_to_comment", { findingId: finding.id, body });
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
  class:opacity-60={finding.resolved}
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
      <span
        class="px-1.5 py-0.5 rounded-full text-[9px] font-medium border border-hairline text-muted shrink-0"
        title="Confidence: {finding.confidence}"
      >{confidenceGlyph(finding.confidence)}</span>
    {/if}
    {#if showLensCategory}
      <span class="text-muted shrink-0">{lensCategory}</span>
    {/if}
    {#if finding.resolved}
      <span class="px-1.5 py-0.5 rounded-full text-[9px] font-medium border border-hairline text-muted shrink-0">
        resolved
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
      <button type="button" onclick={() => (showPromote = true)} class="px-2 py-0.5 rounded text-comment hover:bg-hover flex items-center gap-1">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        Promote to comment
      </button>
    {/if}
    <button type="button" onclick={focusReply} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Reply</button>
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
      {#each mergedReplies as entry, i (entry.key)}
        <div class="px-3 py-2.5 flex gap-2.5 group/row {i > 0 ? 'border-t border-hairline' : ''}">
          <div class="w-6 h-6 rounded-full flex items-center justify-center shrink-0 text-[11px] font-bold {entry.kind === 'ai' ? 'bg-ai/20' : 'bg-accent text-on-accent'}">
            {#if entry.kind === "ai"}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-ai"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
            {:else}
              {(entry.author || "Y")[0].toUpperCase()}
            {/if}
          </div>
          <div class="flex-1 min-w-0 {entry.kind === 'ai' ? 'border-l-2 border-ai pl-2.5' : ''}">
            <div class="text-[11px] font-mono text-muted mb-0.5">
              {#if entry.kind === "ai"}<span class="text-ai font-medium font-sans">AI</span>{:else}<span>{entry.author}</span>{/if}
              {#if entry.timestamp}<span>· {formatTimestamp(entry.timestamp)}</span>{/if}
            </div>
            {#if entry.kind === "ai" && entry.body_markdown === "…thinking"}
              <div class="text-sm text-fg-3 italic animate-pulse">…thinking</div>
            {:else}
              <div class="annotation-body-scroll">
                <MarkdownText text={entry.body_markdown} className="text-sm text-fg-2" />
              </div>
            {/if}
            {#if entry.id && entry.body_markdown !== "…thinking"}
              <ReplyActionBar
                reply={entry}
                rootThreadId={thread?.id ?? null}
                findingId={finding.id}
                isQuestion={thread?.kind === "question"}
                parentSynced={thread?.synced ?? false}
                threadResolved={thread?.resolved ?? false}
                onEdit={entry.editable ? () => openEdit(entry) : undefined}
                onDelete={() => deleteReply(entry.id, entry.origin)}
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
