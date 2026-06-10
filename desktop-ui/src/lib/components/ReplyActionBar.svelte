<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import type { FindingResponseSnapshot, ThreadMessage } from "$lib/types";

  type ReplyLike = (ThreadMessage | FindingResponseSnapshot) & { synced?: boolean };

  interface Props {
    reply: ReplyLike;
    rootThreadId?: string | null;
    findingId?: string | null;
    isQuestion?: boolean;
    parentSynced?: boolean;
    threadResolved?: boolean;
    onEdit?: () => void;
    onDelete: () => void | Promise<void>;
  }

  const {
    reply,
    rootThreadId = null,
    findingId = null,
    isQuestion = false,
    parentSynced = false,
    threadResolved = false,
    onEdit,
    onDelete,
  }: Props = $props();

  const origin = $derived(reply.origin ?? "thread_reply");
  const isFindingResponse = $derived(origin === "finding_response");
  const hasThread = $derived(Boolean(rootThreadId));
  const showPush = $derived(
    !isQuestion &&
      !isFindingResponse &&
      hasThread &&
      origin === "thread_reply" &&
      reply.synced === false &&
      parentSynced,
  );
  const showAskAi = $derived(hasThread || Boolean(findingId));
  const showResolve = $derived(hasThread && !threadResolved && !isQuestion);
  let pushing = $state(false);
  let justCopied = $state(false);

  async function askAi() {
    const focus = reply.body_markdown.trim();
    const prompt = focus
      ? `Focus on this reply:\n\n${focus}\n\nElaborate and answer directly.`
      : "Elaborate on this and answer any question directly.";
    if (rootThreadId) {
      await app.cmd("ask_ai", { threadId: rootThreadId, prompt });
    } else if (findingId) {
      await app.cmd("reply_to_finding", { findingId, body: prompt, aiAssist: true });
    }
  }

  async function validateWithAi() {
    if (findingId) {
      await app.cmd("validate_with_ai", { threadId: rootThreadId, findingId });
    } else if (rootThreadId) {
      await app.cmd("validate_with_ai", { threadId: rootThreadId, findingId: null });
    }
  }

  async function copyReply() {
    await copyToClipboard(reply.body_markdown);
    justCopied = true;
    setTimeout(() => (justCopied = false), 1500);
  }

  async function pushReply() {
    if (!reply.id || pushing) return;
    pushing = true;
    try {
      const activeTab = app.snapshot?.tabs?.find((t) => t.is_active) ?? null;
      const currentWorktree = app.snapshot?.worktrees.find((w) => w.is_current) ?? null;
      const prNumber =
        activeTab?.pr_number ??
        currentWorktree?.pr_number ??
        app.snapshot?.github?.number ??
        app.snapshot?.pr?.number ??
        null;
      await app.cmd("push_github_comment_reply", { replyId: reply.id, prNumber });
    } finally {
      pushing = false;
    }
  }

  async function resolveThread() {
    if (!rootThreadId) return;
    await app.cmd("resolve_thread", { id: rootThreadId });
  }
</script>

<div class="self-start shrink-0 flex flex-wrap gap-0.5 opacity-0 group-hover/row:opacity-100 text-[10px]">
  {#if showAskAi}
    <button type="button" onclick={() => void askAi()} title="Ask AI about this reply" class="px-1 py-0.5 rounded text-fg-3 hover:bg-hover">Ask AI</button>
  {/if}
  {#if findingId || rootThreadId}
    <button type="button" onclick={() => void validateWithAi()} title="Validate with AI" class="px-1 py-0.5 rounded text-ai hover:bg-hover">Validate</button>
  {/if}
  <button type="button" onclick={() => void copyReply()} title="Copy reply" class="px-1 py-0.5 rounded hover:bg-hover {justCopied ? 'text-add-fg' : 'text-fg-3'}">
    {justCopied ? "Copied" : "Copy"}
  </button>
  {#if onEdit}
    <button type="button" onclick={onEdit} title="Edit reply" class="px-1 py-0.5 rounded text-fg-3 hover:bg-hover">Edit</button>
  {/if}
  {#if showPush}
    <button type="button" onclick={() => void pushReply()} disabled={pushing} title="Push only this reply" class="px-1 py-0.5 rounded text-fg-3 hover:bg-hover disabled:opacity-40">
      {pushing ? "…" : "Push"}
    </button>
  {/if}
  {#if showResolve}
    <button type="button" onclick={() => void resolveThread()} title="Resolve thread" class="px-1 py-0.5 rounded text-fg-3 hover:bg-hover">Resolve</button>
  {/if}
  {#if reply.deletable !== false}
    <button type="button" onclick={() => void onDelete()} title="Delete reply" class="px-1 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-del-fg">Delete</button>
  {/if}
</div>
