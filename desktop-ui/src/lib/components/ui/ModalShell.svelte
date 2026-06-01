<script lang="ts">
  import { tick, untrack, type Snippet } from "svelte";
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

  const props: Props = $props();

  let panelEl = $state<HTMLDivElement | null>(null);

  function dataModalValue(): string | true {
    return props.dataModal || true;
  }

  function focusModal() {
    const target = props.focusSelector
      ? panelEl?.querySelector<HTMLElement>(props.focusSelector)
      : null;
    (target ?? panelEl)?.focus({ preventScroll: true });
  }

  function closeFromBackdrop(e: MouseEvent | PointerEvent) {
    if (!(props.closeOnBackdrop ?? true)) return;
    if (e.target !== e.currentTarget) return;
    e.preventDefault();
    e.stopPropagation();
    props.onClose();
  }

  // Track only `open`. Overlay acquire/register and keydown wiring run inside untrack
  // so depth/stack updates cannot retrigger this effect (infinite loop + depth runaway).
  $effect(() => {
    if (!props.open) return;

    const releaseOverlay = untrack(() => overlay.acquire());
    const registration = untrack(() =>
      overlay.registerModal(() => props.onClose()),
    );
    const modalId = registration.id;
    const previousFocus = document.activeElement as HTMLElement | null;
    let cancelled = false;

    function handleWindowKeydown(e: KeyboardEvent) {
      if (!overlay.isTopModal(modalId)) return;
      const closeOnEscape = props.closeOnEscape ?? true;
      if (closeOnEscape && e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        props.onClose();
        return;
      }
      props.onKeydown?.(e);
    }

    window.addEventListener("keydown", handleWindowKeydown, { capture: true });
    void tick().then(() => {
      if (!cancelled) focusModal();
    });

    return () => {
      cancelled = true;
      window.removeEventListener("keydown", handleWindowKeydown, { capture: true });
      untrack(() => {
        registration.unregister();
        releaseOverlay();
      });
      if (previousFocus?.isConnected) {
        queueMicrotask(() => previousFocus.focus({ preventScroll: true }));
      }
    };
  });
</script>

{#if props.open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    data-modal={dataModalValue()}
    class={props.backdropClass ?? "fixed inset-0 z-[250] bg-black/50"}
    style={props.backdropStyle ?? "backdrop-filter: blur(2px);"}
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
    aria-label={props.ariaLabel}
    class={props.panelClass
      ? `fixed z-[251] outline-none ${props.panelClass}`
      : "fixed left-1/2 -translate-x-1/2 top-[15vh] z-[251] bg-card border border-border rounded-lg shadow-2xl overflow-hidden outline-none"}
    style={props.panelStyle ?? ""}
    onpointerdown={(e) => e.stopPropagation()}
    onmousedown={(e) => e.stopPropagation()}
  >
    {@render props.children()}
  </div>
{/if}
