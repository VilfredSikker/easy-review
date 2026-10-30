<script lang="ts">
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { inboxToneStyle } from "$lib/inboxThreads";
  import { tabOpenKeyChoice, type TabOpenRequest } from "$lib/prOpen";

  interface Props {
    request: TabOpenRequest | null;
    tabCount: number;
    /** Label of the tab "Open here" would replace; null when nothing is open. */
    currentTabLabel: string | null;
    onChoose: (newTab: boolean, alwaysNewTab: boolean) => void;
    onClose: () => void;
  }

  const { request, tabCount, currentTabLabel, onChoose, onClose }: Props = $props();

  let remember = $state(false);
  let highlighted = $state<"new" | "here">("new");

  // A string, so a snapshot that rebuilds the request object for the same open
  // does not untick the checkbox the reader just ticked.
  const openKey = $derived(request?.key ?? null);

  $effect(() => {
    if (openKey === null) return;
    remember = false;
    highlighted = "new";
  });

  function choose(newTab: boolean) {
    onChoose(newTab, remember);
  }

  // preventDefault stops the focused button from also firing its own click.
  function handleKeydown(e: KeyboardEvent) {
    const choice = tabOpenKeyChoice(e, e.target instanceof HTMLElement ? e.target : null);
    if (!choice) return;
    e.preventDefault();
    choose(choice === "new-tab");
  }

  const current = $derived(currentTabLabel ? `“${currentTabLabel}”` : "the current tab");
</script>

<ModalShell
  open={request !== null}
  ariaLabel="Open pull request"
  {onClose}
  onKeydown={handleKeydown}
  focusSelector="[data-tm-primary]"
  dataModal="tab-open"
  backdropClass="fixed inset-0 z-[250] bg-bg/55"
  panelClass="left-1/2 -translate-x-1/2 top-[12vh] w-[560px] max-w-[calc(100vw-2rem)] rounded-xl bg-card border border-border shadow-2xl overflow-hidden"
>
  {#if request}
    <div class="grid grid-cols-[26px_minmax(0,1fr)_auto] gap-3 items-start pt-3.5 pb-2.5 pl-4 pr-3.5">
      <span class="w-[26px] h-[26px] rounded-[7px] grid place-items-center" style={inboxToneStyle(request.tone)}>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d={request.iconPath} /></svg>
      </span>
      <div class="min-w-0">
        {#if request.context}
          <div class="text-[11.5px] text-muted truncate">{request.context}</div>
        {/if}
        <div class="text-[15px] leading-snug font-semibold text-fg mt-0.5">{request.title}</div>
      </div>
      <button
        type="button"
        onclick={onClose}
        aria-label="Close"
        class="w-6 h-6 rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover"
      >
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </div>

    <div class="px-4 pb-3.5 grid gap-3">
      <div class="flex gap-2 text-[12px] pl-[38px]">
        <span class="font-mono text-fg-2">{request.ref}</span>
        <span class="text-muted">·</span>
        <span class="text-muted">{tabCount} {tabCount === 1 ? "tab" : "tabs"} open</span>
      </div>

      {#if request.quote}
        <div class="px-3 py-2 border border-hairline rounded-[9px] bg-bg text-[13px] leading-normal text-fg-2 whitespace-pre-wrap break-words max-h-32 overflow-y-auto">
          {request.quote}
        </div>
      {/if}

      <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
        <button
          type="button"
          onclick={() => choose(true)}
          onmouseenter={() => (highlighted = "new")}
          class="relative grid gap-0.5 text-left px-3 py-2.5 rounded-[9px] border text-[12px] {highlighted === 'new' ? 'border-accent ring-1 ring-accent' : 'border-border hover:bg-hover'}"
        >
          <b class="text-[13px] text-fg font-semibold">Open in new tab</b>
          <span class="text-muted">Keeps {current} as it is.</span>
          <kbd class="absolute top-2 right-2.5 font-mono text-[10px] text-muted">↵</kbd>
        </button>
        <button
          type="button"
          onclick={() => choose(false)}
          onmouseenter={() => (highlighted = "here")}
          class="relative grid gap-0.5 text-left px-3 py-2.5 rounded-[9px] border text-[12px] {highlighted === 'here' ? 'border-accent ring-1 ring-accent' : 'border-border hover:bg-hover'}"
        >
          <b class="text-[13px] text-fg font-semibold">Open here</b>
          <span class="text-muted">Replaces {current}. Its review state is saved.</span>
          <kbd class="absolute top-2 right-2.5 font-mono text-[10px] text-muted">⌘↵</kbd>
        </button>
      </div>

      <label class="flex items-center gap-2 text-[12px] text-fg-3">
        <input type="checkbox" bind:checked={remember} class="accent-accent" />
        Always open in a new tab, don't ask again
      </label>
    </div>

    <div class="flex justify-end gap-2 px-3.5 py-2.5 border-t border-hairline bg-bg">
      <button type="button" onclick={onClose} class="px-3 py-1.5 rounded-md text-[12px] text-fg-3 hover:bg-hover hover:text-fg">
        Cancel <span class="font-mono text-[10.5px] text-muted">esc</span>
      </button>
      <button type="button" onclick={() => choose(false)} class="px-3 py-1.5 rounded-md border border-border text-[12px] text-fg-2 hover:bg-hover">
        Open here
      </button>
      <button
        type="button"
        data-tm-primary
        onclick={() => choose(true)}
        class="px-3 py-1.5 rounded-md bg-accent text-on-accent text-[12px] font-medium hover:bg-accent-hover"
      >
        Open in new tab
      </button>
    </div>
  {/if}
</ModalShell>
