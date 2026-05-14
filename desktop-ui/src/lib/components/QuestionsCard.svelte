<script lang="ts">
  import type { AiSnapshot } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import Card from "$lib/components/ui/Card.svelte";
  import SectionLabel from "$lib/components/ui/SectionLabel.svelte";
  import Pill from "$lib/components/ui/Pill.svelte";
  import Button from "$lib/components/ui/Button.svelte";
  import { navigateToThread } from "$lib/dom";

  interface Props {
    ai: AiSnapshot;
  }

  const { ai }: Props = $props();

  const questionThreads = $derived(ai.threads.filter((t) => t.kind === "question"));

  function basename(p: string): string {
    const i = p.lastIndexOf("/");
    return i === -1 ? p : p.slice(i + 1);
  }
</script>

<Card>
  <div class="flex items-center justify-between mb-3">
    <div class="flex items-center gap-2">
      <SectionLabel>Questions</SectionLabel>
      <Pill textColor="text-muted">
        <svg width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3"><rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
        private
      </Pill>
    </div>
    <span class="flex items-center gap-1 text-[10px] mono text-question"><svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>{questionThreads.length}</span>
  </div>

  <div class="space-y-1 mb-3">
    {#each questionThreads as thread (thread.id)}
      <button
        onclick={() => navigateToThread(thread)}
        class="w-full text-left text-sm border-l-2 border-question pl-2 pr-1 py-1.5 hover:bg-bg flex flex-col gap-0.5 group"
      >
        <div class="text-[11px] font-mono text-muted flex items-center gap-1.5">
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="#fde047" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3M12 17h.01"/></svg>
          <span>{basename(thread.file)}:{thread.line}</span>
          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="ml-auto opacity-0 group-hover:opacity-100 transition text-accent"><path d="M7 17L17 7M7 7h10v10"/></svg>
        </div>
        <div class="text-fg-2 text-left">{thread.root.body_markdown}</div>
      </button>
    {/each}
  </div>

  <div class="text-[11px] text-muted mb-2 leading-snug">Questions stay on your machine. Use them for personal review notes or routing to an AI assistant.</div>

  <div class="grid grid-cols-2 gap-2">
    <Button
      variant="primary"
      class="flex items-center justify-center gap-1.5 normal-case"
      disabled={questionThreads.length === 0}
      onclick={() => {
        const t = questionThreads[questionThreads.length - 1];
        if (t) app.cmd("ask_ai", { threadId: t.id, prompt: "" });
      }}
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M12 2l3 7h7l-5.5 4 2 7L12 16l-6.5 4 2-7L2 9h7z"/></svg>
      Ask AI
    </Button>
    <Button
      variant="secondary"
      class="flex items-center justify-center gap-1.5 normal-case"
      disabled={questionThreads.length === 0}
      onclick={() => {
        const t = questionThreads[questionThreads.length - 1];
        if (t) app.cmd("promote_to_comment", { id: t.id });
      }}
    >
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
      Promote to comment
    </Button>
  </div>
</Card>
