import { invoke } from "@tauri-apps/api/core";

export function startWindowDrag(e: MouseEvent) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement | null;
  if (target?.closest(".titlebar-no-drag, .tabstrip-no-drag")) return;

  invoke("start_window_drag").catch(() => {});
}
