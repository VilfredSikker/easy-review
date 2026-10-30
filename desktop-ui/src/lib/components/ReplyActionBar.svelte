<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { copyToClipboard } from "$lib/clipboard";
  import { resolveActivePrNumber } from "$lib/prUrl";
  import AnnotationOverflowMenu from "$lib/components/AnnotationOverflowMenu.svelte";
  import type { OverflowItem } from "$lib/annotationActions";
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

  // Copy, Push, Edit and Validate are rarer than Ask AI / Resolve / Delete, so
  // they sit behind the menu rather than crowding every reply.
  const overflow = $derived.by((): OverflowItem[] => {
    const items: OverflowItem[] = [
      { id: "copy", label: "Copy reply", onselect: () => void copyReply() },
    ];
    if (onEdit) items.push({ id: "edit", label: "Edit", onselect: onEdit });
    if (showPush) {
      items.push({
        id: "push",
        label: pushing ? "Pushing…" : "Push only this",
        title: "Push only this reply",
        disabled: pushing,
        onselect: () => void pushReply(),
      });
    }
    if (findingId || rootThreadId) {
      items.push({ id: "validate", label: "Validate with AI", onselect: () => void validateWithAi() });
    }
    return items;
  });

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
  }

  async function pushReply() {
    if (!reply.id || pushing) return;
    pushing = true;
    try {
      const prNumber = resolveActivePrNumber(app.snapshot);
      await app.cmd("push_github_comment_reply", { replyId: reply.id, prNumber });
    } finally {
      pushing = false;
    }
  }

  function resolveThread() {
    if (!rootThreadId) return;
    void app.cmd("resolve_thread", { id: rootThreadId });
  }
</script>

<div class="mt-1 -ml-1 flex flex-wrap items-center gap-0.5 opacity-0 group-hover/row:opacity-100 focus-within:opacity-100 text-[11px]">
  {#if showAskAi}
    <button type="button" onclick={() => void askAi()} title="Ask AI about this reply" class="px-1.5 py-0.5 rounded text-accent hover:bg-accent-soft">✦ {isQuestion ? "Elaborate" : "Ask AI"}</button>
  {/if}
  {#if showResolve}
    <button type="button" onclick={() => void resolveThread()} title="Resolve thread" class="px-1.5 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-fg-2">Resolve</button>
  {/if}
  {#if reply.deletable !== false}
    <button type="button" onclick={() => void onDelete()} title="Delete reply" class="px-1.5 py-0.5 rounded text-fg-3 hover:bg-hover hover:text-del-fg">Delete</button>
  {/if}
  <AnnotationOverflowMenu items={overflow} label="More reply actions" />
</div>
