import { getCurrentWindow } from "@tauri-apps/api/window";
import { browserSuspendForOverlay } from "./browserHost";

let nextModalId = 1;

type ModalEntry = {
  id: number;
  onClose: () => void;
};

/** Blocks the native review-browser webview while app modals are open (z-index cannot cover it). */
class OverlayStore {
  #depth = $state(0);
  /** Plain array — stack changes must not retrigger unrelated Svelte effects. */
  #modalStack: ModalEntry[] = [];

  get blocksNativeBrowser(): boolean {
    return this.#depth > 0;
  }

  /** Call the returned function when the overlay closes. */
  acquire(): () => void {
    const first = this.#depth === 0;
    this.#depth += 1;
    if (first) void prepareOverlayFocus();
    return () => {
      this.#depth = Math.max(0, this.#depth - 1);
    };
  }

  registerModal(onClose: () => void): { id: number; unregister: () => void } {
    const id = nextModalId++;
    this.#modalStack.push({ id, onClose });
    return {
      id,
      unregister: () => {
        this.#modalStack = this.#modalStack.filter((modal) => modal.id !== id);
      },
    };
  }

  isTopModal(id: number): boolean {
    return this.#modalStack[this.#modalStack.length - 1]?.id === id;
  }

  /** Invoke the top modal's onClose. Returns true if a modal was dismissed. */
  dismissTopModal(): boolean {
    const top = this.#modalStack[this.#modalStack.length - 1];
    if (!top) return false;
    top.onClose();
    return true;
  }
}

export const overlay = new OverlayStore();

/**
 * Destroy review child webviews and focus the main window before showing a modal.
 * `hide()` is unreliable on macOS — suspended webviews can still steal clicks/focus.
 */
export async function prepareOverlayFocus(): Promise<void> {
  try {
    await browserSuspendForOverlay();
    await getCurrentWindow().setFocus();
  } catch {
    // Not running inside Tauri (storybook / tests).
  }
}
