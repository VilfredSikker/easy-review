import { NoticeFeed } from "$lib/protocol/notices";
import { v1ServiceRequested } from "$lib/v1ServiceFlag";
import { app, TOAST_LONG_MS } from "./app.svelte";

const feed = new NoticeFeed((notice) =>
  app.showToast(notice.level, notice.message, notice.long ? TOAST_LONG_MS : undefined),
);
let unlisten: Promise<() => void> | null = null;

/** Shows the backend's notices as toasts. Only the `v1-service` build emits them. */
export function startNotices(): void {
  if (!v1ServiceRequested() || unlisten) return;
  unlisten = feed.listen();
}

export function stopNotices(): void {
  void unlisten?.then((stop) => stop());
  unlisten = null;
}
