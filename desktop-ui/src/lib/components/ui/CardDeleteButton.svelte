<script lang="ts">
  interface Props {
    /** Accessible label when idle (e.g. "Discard triage"). */
    label: string;
    /** Shown after first click (e.g. "Click again to confirm"). */
    confirmLabel?: string;
    onDelete: () => void | Promise<void>;
  }

  const {
    label,
    confirmLabel = "Click again to confirm",
    onDelete,
  }: Props = $props();

  let pending = $state(false);
  let timer: ReturnType<typeof setTimeout> | null = null;

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  async function handleClick(e: MouseEvent) {
    e.stopPropagation();
    if (!pending) {
      pending = true;
      clearTimer();
      timer = setTimeout(() => {
        pending = false;
        timer = null;
      }, 3000);
      return;
    }
    clearTimer();
    pending = false;
    await onDelete();
  }
</script>

<button
  type="button"
  class="shrink-0 rounded p-1 text-muted opacity-0 transition-opacity
    group-hover:opacity-100 hover:bg-del-bg hover:text-del-fg
    {pending ? 'opacity-100 text-del-fg' : ''}"
  title={pending ? confirmLabel : label}
  aria-label={pending ? confirmLabel : label}
  onclick={handleClick}
>
  {#if pending}
    <span class="text-[10px] whitespace-nowrap px-0.5">{confirmLabel}</span>
  {:else}
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
      <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/>
    </svg>
  {/if}
</button>
