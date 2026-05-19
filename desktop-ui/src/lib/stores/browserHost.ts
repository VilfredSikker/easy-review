import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const BROWSER_MESSAGE_EVENT = "browser://message";

/** Subscribe to messages from the review-browser content script. */
export async function listenBrowserMessages(
  handler: (payload: Record<string, unknown>) => void,
): Promise<UnlistenFn> {
  return listen<Record<string, unknown>>(BROWSER_MESSAGE_EVENT, (event) => {
    if (event.payload && typeof event.payload === "object") {
      handler(event.payload);
    }
  });
}

export async function browserEnsure(url: string): Promise<void> {
  await invoke("browser_ensure", { url });
}

export async function browserHide(): Promise<void> {
  await invoke("browser_hide");
}

export async function browserNavigate(url: string): Promise<void> {
  await invoke("browser_navigate", { url });
}

export async function browserSetBounds(
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<void> {
  await invoke("browser_set_bounds", { x, y, width, height });
}

export async function browserSendToPage(payload: Record<string, unknown>): Promise<void> {
  await invoke("browser_send_to_page", { payload });
}

export async function browserSetAnnotateMode(active: boolean): Promise<void> {
  await invoke("browser_set_annotate_mode", { active });
}
