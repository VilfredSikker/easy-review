<script lang="ts">
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    file: string;
    hunk_idx: number;
    line_num: number | null;
    onclose: () => void;
  }

  const { file, hunk_idx, line_num, onclose }: Props = $props();

  type Tab = "comment" | "question";
  let activeTab = $state<Tab>("comment");
  let text = $state("");

  const canSubmit = $derived(text.trim().length > 0);

  async function submit() {
    if (!canSubmit) return;
    const command = activeTab === "comment" ? "add_comment" : "add_question";
    await app.cmd(command, {
      file,
      hunk_idx,
      line_num,
      text: text.trim(),
    });
    onclose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      submit();
    }
  }
</script>

<div class="border-t border-ink-600/60 bg-ink-900 font-mono text-[12px] shrink-0">
  <!-- Tab row -->
  <div class="flex border-b border-ink-700/60">
    <button
      class="px-4 py-1.5 text-[11px] uppercase tracking-wide transition-colors
        {activeTab === 'comment'
          ? 'text-comment border-b-2 border-comment bg-ink-850'
          : 'text-ink-400 hover:text-ink-200'}"
      onclick={() => (activeTab = "comment")}
    >
      Comment
    </button>
    <button
      class="px-4 py-1.5 text-[11px] uppercase tracking-wide transition-colors
        {activeTab === 'question'
          ? 'text-question border-b-2 border-question bg-ink-850'
          : 'text-ink-400 hover:text-ink-200'}"
      onclick={() => (activeTab = "question")}
    >
      Question
    </button>
  </div>

  <!-- Textarea -->
  <textarea
    class="w-full resize-none bg-ink-900 text-ink-100 px-3 py-2 text-[12px] leading-[1.4]
           placeholder:text-ink-500 outline-none border-b border-ink-700/60 block"
    rows={3}
    placeholder={activeTab === "comment" ? "Add a comment…" : "Ask a question…"}
    bind:value={text}
    onkeydown={handleKeydown}
  ></textarea>

  <!-- Action row -->
  <div class="flex items-center justify-end gap-2 px-3 py-1.5">
    <button
      class="text-[12px] text-ink-400 hover:text-ink-200 transition-colors"
      onclick={onclose}
    >
      Cancel
    </button>
    <button
      class="text-[12px] px-3 py-1 rounded transition-colors
        {canSubmit
          ? 'bg-accent/20 text-accent hover:bg-accent/30'
          : 'text-ink-600 cursor-not-allowed'}"
      disabled={!canSubmit}
      onclick={submit}
    >
      Add
    </button>
  </div>
</div>
