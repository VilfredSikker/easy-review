<script lang="ts">
  import type { ToastMessage } from "$lib/stores/app.svelte";
  import { app } from "$lib/stores/app.svelte";

  interface Props {
    toast: ToastMessage;
  }
  const { toast }: Props = $props();

  const RESUME_MS = 1_800;

  let expanded = $state(false);
  let justCopied = $state(false);
  let msgEl = $state<HTMLDivElement | null>(null);
  let overflowing = $state(false);

  // Check overflow after render or when expanded toggles
  $effect(() => {
    expanded; // track
    if (!msgEl) return;
    overflowing = msgEl.scrollHeight - 2 > msgEl.clientHeight;
  });

  const isPersist = $derived(toast.persist ?? toast.kind === "error");
  const isMultiline = $derived(toast.kind === "error" || toast.kind === "warn");

  interface KindStyle {
    ruleClass: string;
    iconPath: string;
    iconClass: string;
  }

  function kindStyle(kind: ToastMessage["kind"]): KindStyle {
    switch (kind) {
      case "success":
        return {
          ruleClass: "border-l-add-fg",
          iconPath: "M22 11.08V12a10 10 0 1 1-5.93-9.14M22 4 12 14.01l-3-3",
          iconClass: "text-add-fg",
        };
      case "warn":
        return {
          ruleClass: "border-l-[#fbbf24]",
          iconPath:
            "M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0zM12 9v4M12 17h.01",
          iconClass: "text-[#fbbf24]",
        };
      case "error":
        return {
          ruleClass: "border-l-del-fg",
          iconPath: "M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10zM15 9l-6 6M9 9l6 6",
          iconClass: "text-del-fg",
        };
      default:
        return {
          ruleClass: "border-l-periwinkle",
          iconPath:
            "M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10zM12 8h.01M12 12v4",
          iconClass: "text-periwinkle",
        };
    }
  }

  const ks = $derived(kindStyle(toast.kind));

  function onMouseEnter() {
    if (!isPersist) app.pauseToast(toast.id);
  }

  function onMouseLeave() {
    if (!isPersist) app.resumeToast(toast.id, RESUME_MS);
  }

  async function copyMessage() {
    try {
      await navigator.clipboard.writeText(toast.message);
    } catch {
      const ta = document.createElement("textarea");
      ta.value = toast.message;
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
      } finally {
        document.body.removeChild(ta);
      }
    }
    justCopied = true;
    setTimeout(() => (justCopied = false), 1_400);
  }
</script>

<!-- svelte-ignore a11y_mouse_events_have_key_events -->
<div
  role={toast.kind === "error" || toast.kind === "warn" ? "alert" : "status"}
  onmouseenter={onMouseEnter}
  onmouseleave={onMouseLeave}
  class="pointer-events-auto flex gap-2.5 w-[min(480px,calc(100vw-32px))] px-3 py-2 rounded bg-ink-800 border border-ink-500/40 border-l-[3px] shadow-[0_8px_24px_rgba(0,0,0,0.35),0_2px_6px_rgba(0,0,0,0.25)] font-mono text-xs animate-[toast-in_180ms_ease-out] {ks.ruleClass} {isMultiline ? 'items-start' : 'items-center'}"
>
  <!-- Kind icon -->
  <svg
    width="13"
    height="13"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    class="shrink-0 {ks.iconClass} {isMultiline ? 'mt-0.5' : ''}"
  >
    <path d={ks.iconPath} />
  </svg>

  <!-- Body -->
  <div class="flex-1 min-w-0 flex flex-col gap-1.5">
    <!-- Message text -->
    <div
      bind:this={msgEl}
      style:overflow-wrap="anywhere"
      style:white-space={isMultiline ? "normal" : "nowrap"}
      style:overflow={isMultiline ? "hidden" : "visible"}
      style:max-height={isMultiline
        ? expanded
          ? "280px"
          : "calc(3 * 1.45em)"
        : undefined}
      style:overflow-y={expanded ? "auto" : "hidden"}
      style:-webkit-line-clamp={!isMultiline ? undefined : expanded ? "unset" : "3"}
      style:-webkit-box-orient={!isMultiline ? undefined : "vertical"}
      style:display={!isMultiline ? undefined : "-webkit-box"}
      class="leading-[1.45] text-ink-100"
    >
      {toast.message}
    </div>

    <!-- Footer: action + show-more + copy (only when multiline or action present) -->
    {#if isMultiline || toast.action}
      <div class="flex items-center gap-2">
        {#if toast.action}
          <button
            type="button"
            onclick={toast.action.onClick}
            class="border-0 bg-transparent p-0 font-mono text-xs font-semibold cursor-pointer {ks.iconClass}"
          >
            {toast.action.label}
          </button>
        {/if}
        {#if isMultiline && overflowing}
          <button
            type="button"
            onclick={() => (expanded = !expanded)}
            class="border-0 bg-transparent p-0 font-mono text-[11px] text-muted cursor-pointer inline-flex items-center gap-1"
          >
            <svg
              width="9"
              height="9"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              class="transition-transform {expanded ? 'rotate-180' : ''}"
            >
              <polyline points="6 9 12 15 18 9" />
            </svg>
            {expanded ? "Show less" : "Show more"}
          </button>
        {/if}
        <div class="flex-1"></div>
        {#if isMultiline}
          <button
            type="button"
            onclick={copyMessage}
            title={justCopied ? "Copied" : "Copy message"}
            class="border-0 bg-transparent px-1 py-0.5 rounded font-mono text-[11px] cursor-pointer inline-flex items-center gap-1 hover:bg-ink-700 {justCopied ? ks.iconClass : 'text-muted'}"
          >
            {#if justCopied}
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
                <polyline points="20 6 9 17 4 12" />
              </svg>
              Copied
            {:else}
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <rect x="9" y="9" width="13" height="13" rx="2" />
                <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
              </svg>
              Copy
            {/if}
          </button>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Close × (persistent toasts only) -->
  {#if isPersist}
    <button
      type="button"
      onclick={() => app.closeToast(toast.id)}
      title="Dismiss"
      aria-label="Dismiss"
      class="shrink-0 w-5 h-5 rounded flex items-center justify-center border-0 bg-transparent text-muted hover:bg-ink-700 hover:text-fg cursor-pointer {isMultiline ? 'mt-0.5' : ''}"
    >
      <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <line x1="18" y1="6" x2="6" y2="18" />
        <line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  {/if}
</div>
