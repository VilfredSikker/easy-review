import { getCurrentWindow } from "@tauri-apps/api/window";
import { browserSuspendForOverlay } from "./browserHost";

let nextModalId = 1;

/** Blocks the native review-browser webview while app modals are open (z-index cannot cover it). */
class OverlayStore {
  #depth = $state(0);
  #modalStack = $state<number[]>([]);

  get blocksNativeBrowser(): boolean {
    return this.#depth > 0;
  }

  /** Call the returned function when the overlay closes. */
  acquire(): () => void {
    this.#depth += 1;
    void prepareOverlayFocus();
    return () => {
      this.#depth = Math.max(0, this.#depth - 1);
    };
  }

  registerModal(): { id: number; unregister: () => void } {
    const id = nextModalId++;
    this.#modalStack = [...this.#modalStack, id];
    return {
      id,
      unregister: () => {
        this.#modalStack = this.#modalStack.filter((modalId) => modalId !== id);
      },
    };
  }

  isTopModal(id: number): boolean {
    return this.#modalStack[this.#modalStack.length - 1] === id;
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
