<script lang="ts">
  import type { FlatFinding } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import PromoteModal from "$lib/components/PromoteModal.svelte";

  interface Props {
    finding: FlatFinding;
  }

  const { finding }: Props = $props();

  const severityColor = $derived(
    finding.severity === "high" ? "#ef4444"
    : finding.severity === "med" ? "#fbbf24"
    : "#60a5fa",
  );

  const isPromoted = $derived(finding.promoted_to != null);

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
    await app.cmd("reply_to_finding", {
      findingId: finding.id,
      body: "Elaborate on this and answer any question directly.",
      aiAssist: true,
    });
    replyText = "";
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
</script>

<div
  id="finding-{finding.id}"
  class="mx-4 my-3 border rounded-lg overflow-hidden font-sans scroll-mt-16"
  style="border-color: {severityColor}4d; background: {severityColor}0a;"
>
  <!-- Header -->
  <div class="px-3 py-2 border-b border-hairline flex items-center gap-2 text-xs">
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke={severityColor} stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M8 12l3 3 5-6"/></svg>
    <span class="font-medium" style="color: {severityColor}">AI finding</span>
    <span class="px-1.5 py-0.5 rounded-full text-[9px] uppercase tracking-wider font-medium" style="background: {severityColor}26; color: {severityColor}">
      {finding.severity}
    </span>
    {#if finding.line !== null}
      <span class="text-muted">· line {finding.line}</span>
    {/if}
    <span class="ml-auto text-[10px] mono text-muted">AI</span>
  </div>

  <!-- Body -->
  <div class="px-3 py-2 text-sm text-fg-2 whitespace-pre-wrap">
    {finding.title}
    {#if finding.message_markdown}
      <div class="text-fg-3 mt-1">{finding.message_markdown}</div>
    {/if}
  </div>

  <!-- Action footer -->
  <div class="px-3 py-1.5 border-t border-hairline flex items-center gap-2 text-[11px] flex-wrap">
    {#if !isPromoted}
      <button onclick={() => (showPromote = true)} class="px-2 py-0.5 rounded text-comment hover:bg-hover flex items-center gap-1">
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        Promote to comment
      </button>
    {/if}
    <button onclick={focusReply} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Reply</button>
    <button onclick={dismiss} class="px-2 py-0.5 rounded text-fg-3 hover:bg-hover">Dismiss</button>
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
