<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";
  import { overlay } from "$lib/stores/overlay.svelte";

  interface Props {
    open: boolean;
    children: Snippet;
    ariaLabel: string;
    onClose: () => void;
    onKeydown?: (e: KeyboardEvent) => void;
    closeOnEscape?: boolean;
    closeOnBackdrop?: boolean;
    focusSelector?: string;
    backdropClass?: string;
    backdropStyle?: string;
    panelClass?: string;
    panelStyle?: string;
    dataModal?: string;
  }

  const {
    open,
    children,
    ariaLabel,
    onClose,
    onKeydown,
    closeOnEscape = true,
    closeOnBackdrop = true,
    focusSelector,
    backdropClass = "fixed inset-0 z-[250] bg-black/50",
    backdropStyle = "backdrop-filter: blur(2px);",
    panelClass = "fixed left-1/2 -translate-x-1/2 top-[15vh] z-[251] bg-card border border-border rounded-lg shadow-2xl overflow-hidden outline-none",
    panelStyle = "",
    dataModal = "",
  }: Props = $props();

  let panelEl = $state<HTMLDivElement | null>(null);
  let modalId = 0;

  function dataModalValue(): string | true {
    return dataModal || true;
  }

  function focusModal() {
    const target = focusSelector
      ? panelEl?.querySelector<HTMLElement>(focusSelector)
      : null;
    (target ?? panelEl)?.focus({ preventScroll: true });
  }

  function closeFromBackdrop(e: MouseEvent | PointerEvent) {
    if (!closeOnBackdrop || e.target !== e.currentTarget) return;
    e.preventDefault();
    e.stopPropagation();
    onClose();
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    if (!open || !overlay.isTopModal(modalId)) return;
    if (closeOnEscape && e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    onKeydown?.(e);
  }

  $effect(() => {
    if (!open) return;
    const releaseOverlay = overlay.acquire();
    const registration = overlay.registerModal();
    const previousFocus = document.activeElement as HTMLElement | null;
    modalId = registration.id;
    let cancelled = false;
    void tick().then(() => {
      if (!cancelled) focusModal();
    });
    return () => {
      cancelled = true;
      registration.unregister();
      releaseOverlay();
      if (previousFocus?.isConnected) {
        queueMicrotask(() => previousFocus.focus({ preventScroll: true }));
      }
      modalId = 0;
    };
  });

  onMount(() => {
    window.addEventListener("keydown", handleWindowKeydown, { capture: true });
    return () => window.removeEventListener("keydown", handleWindowKeydown, { capture: true });
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-modal={dataModalValue()}
    class={backdropClass}
    style={backdropStyle}
    role="presentation"
    onpointerdown={closeFromBackdrop}
    onmousedown={closeFromBackdrop}
    onclick={closeFromBackdrop}
  ></div>

  <div
    bind:this={panelEl}
    data-modal={dataModalValue()}
    tabindex="-1"
    role="dialog"
    aria-modal="true"
    aria-label={ariaLabel}
    class={panelClass}
    style={panelStyle}
    onpointerdown={(e) => e.stopPropagation()}
    onmousedown={(e) => e.stopPropagation()}
  >
    {@render children()}
  </div>
{/if}
