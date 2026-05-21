import { prepareOverlayFocus } from "./overlay.svelte";
import { overlay } from "./overlay.svelte";

/** Shared reactive state — use exported $state object so all readers re-render. */
export const prUrlModal = $state({
  open: false,
  url: "",
  submitting: false,
});

let overlayRelease: (() => void) | null = null;

export function isPrUrlModalOpen(): boolean {
  return prUrlModal.open;
}

export async function openPrUrlModal(): Promise<void> {
  await prepareOverlayFocus();
  overlayRelease?.();
  overlayRelease = overlay.acquire();
  prUrlModal.url = "";
  prUrlModal.submitting = false;
  prUrlModal.open = true;
}

export function closePrUrlModal(): void {
  prUrlModal.open = false;
  prUrlModal.url = "";
  prUrlModal.submitting = false;
  overlayRelease?.();
  overlayRelease = null;
}
