<script lang="ts" module>
  let openModal: ((scope: string, reviewerKinds: string[], paths: string[]) => void) | null = null;

  export function openProfessorFocusModal(
    scope: string,
    reviewerKinds: string[],
    paths: string[],
  ): void {
    openModal?.(scope, reviewerKinds, paths);
  }
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "$lib/stores/app.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import Button from "$lib/components/ui/Button.svelte";

  let open = $state(false);
  let focusText = $state("");
  let scope = $state<string | null>(null);
  let reviewerKinds = $state<string[]>([]);
  let paths = $state<string[]>([]);
  let submitting = $state(false);

  function close() {
    open = false;
    focusText = "";
    scope = null;
    reviewerKinds = [];
    paths = [];
    submitting = false;
  }

  function openFromOutside(s: string, kinds: string[], p: string[]) {
    scope = s;
    reviewerKinds = kinds;
    paths = p;
    focusText = "";
    open = true;
  }

  async function run(skipFocus: boolean) {
    if (!scope || submitting) return;
    submitting = true;
    try {
      close();
      await app.cmd("run_ai_scoped_review", {
        scope,
        paths,
        reviewerKinds,
        focusPrompt: skipFocus || !focusText.trim() ? null : focusText.trim(),
      });
    } finally {
      submitting = false;
    }
  }

  onMount(() => {
    openModal = openFromOutside;
    return () => {
      openModal = null;
    };
  });
</script>

<ModalShell
  {open}
  ariaLabel="Professor focus"
  onClose={close}
  panelClass="fixed left-1/2 -translate-x-1/2 top-[18vh] z-[253] bg-ink-800 border border-ink-500 rounded-lg shadow-2xl w-[min(520px,calc(100vw-2rem))] flex flex-col overflow-hidden outline-none"
>
  <div class="px-4 pt-3 pb-2 border-b border-ink-600">
    <span class="text-xs text-ink-300 font-mono">Professor — optional focus</span>
    <p class="text-[11px] text-ink-400 mt-1">What should the learning insights emphasize?</p>
  </div>
  <div class="px-4 py-3">
    <textarea
      bind:value={focusText}
      rows={4}
      placeholder="e.g. How does authentication flow through these changes?"
      class="w-full rounded-md border border-ink-500 bg-ink-900 px-3 py-2 text-sm text-ink-100 placeholder:text-ink-500 outline-none focus:border-accent resize-y font-sans"
    ></textarea>
  </div>
  <div class="px-4 py-3 border-t border-ink-600 flex items-center justify-end gap-2">
    <Button variant="ghost" onclick={close}>Cancel</Button>
    <Button variant="ghost" disabled={submitting} onclick={() => void run(true)}>Skip</Button>
    <Button variant="primary" disabled={submitting} onclick={() => void run(false)}>Run</Button>
  </div>
</ModalShell>
