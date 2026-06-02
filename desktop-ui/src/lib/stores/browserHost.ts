import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { app } from "./app.svelte";

export const BROWSER_MESSAGE_EVENT = "browser://message";

function activeTabIdx(tabIdx?: number): number {
  return tabIdx ?? app.snapshot?.active_tab ?? 0;
}

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

export async function browserEnsure(url: string, tabIdx?: number): Promise<void> {
  await invoke("browser_ensure", { url, tabIdx: activeTabIdx(tabIdx) });
}

export async function browserHide(tabIdx?: number): Promise<void> {
  await invoke("browser_hide", { tabIdx: tabIdx ?? null });
}

/** Destroy all review child webviews so modals receive clicks (macOS). */
export async function browserSuspendForOverlay(): Promise<void> {
  await invoke("browser_suspend_for_overlay");
}

export async function browserNavigate(url: string, tabIdx?: number): Promise<void> {
  await invoke("browser_navigate", { url, tabIdx: activeTabIdx(tabIdx) });
}

export async function browserSetBounds(
  x: number,
  y: number,
  width: number,
  height: number,
  tabIdx?: number,
): Promise<void> {
  await invoke("browser_set_bounds", {
    x,
    y,
    width,
    height,
    tabIdx: activeTabIdx(tabIdx),
  });
}

export async function browserSendToPage(
  payload: Record<string, unknown>,
  tabIdx?: number,
): Promise<void> {
  await invoke("browser_send_to_page", {
    payload,
    tabIdx: activeTabIdx(tabIdx),
  });
}

export async function browserSetAnnotateMode(active: boolean, tabIdx?: number): Promise<void> {
  await invoke("browser_set_annotate_mode", {
    active,
    tabIdx: activeTabIdx(tabIdx),
  });
}

export async function browserReload(tabIdx?: number): Promise<void> {
  await invoke("browser_reload", { tabIdx: activeTabIdx(tabIdx) });
}
