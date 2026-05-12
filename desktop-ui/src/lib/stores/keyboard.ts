import { getCurrentWindow } from "@tauri-apps/api/window";
import { app } from "./app.svelte";

export function initKeyboard(): () => void {
  function handler(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;

    if (e.ctrlKey && e.key === "q") {
      getCurrentWindow().close();
      return;
    }

    if ((e.metaKey || e.ctrlKey) && e.key === "k") {
      console.log("⌘K palette (stub)");
      return;
    }

    if (!e.ctrlKey && !e.metaKey && !e.altKey) {
      if (e.key === "[") {
        console.log("keyboard: toggle left");
        app.togglePanel("left");
      } else if (e.key === "\\") {
        console.log("keyboard: toggle tree");
        app.togglePanel("tree");
      } else if (e.key === "]") {
        console.log("keyboard: toggle right");
        app.togglePanel("right");
      } else if (e.key === "j") {
        app.cmd("next_file");
      } else if (e.key === "k") {
        app.cmd("prev_file");
      } else if (e.key === "U") {
        app.cmd("jump_to_unreviewed");
      }
    }
  }

  window.addEventListener("keydown", handler);
  return () => window.removeEventListener("keydown", handler);
}
