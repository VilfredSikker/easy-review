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
      // command palette stub
      return;
    }

    if (e.ctrlKey || e.metaKey || e.altKey) return;

    switch (e.key) {
      // Panel toggles
      case "[": app.togglePanel("left"); break;
      case "\\": app.togglePanel("tree"); break;
      case "]": app.togglePanel("right"); break;

      // File navigation
      case "j": app.cmd("next_file"); break;
      case "k": app.cmd("prev_file"); break;
      case "U": app.cmd("jump_to_unreviewed"); break;

      // Hunk navigation
      case "n": app.cmd("next_hunk"); break;
      case "N": app.cmd("prev_hunk"); break;

      // Reviewed
      case "r": app.cmd("toggle_reviewed"); break;

      // Expand / collapse compacted file
      case "Enter": app.cmd("toggle_compacted"); break;

      // Refresh diff
      case "R": app.cmd("refresh_diff"); break;

      // Open in editor
      case "e": {
        import("@tauri-apps/api/core").then(({ invoke }) => {
          invoke("open_in_editor").catch(() => {});
        });
        break;
      }

      // Scope / mode switching
      case "1": app.cmd("set_mode", { mode: "branch" }); break;
      case "2": app.cmd("set_mode", { mode: "unstaged" }); break;
      case "3": app.cmd("set_mode", { mode: "staged" }); break;
      case "4": app.cmd("set_mode", { mode: "history" }); break;

      // GitHub sync
      case "g": app.cmd("pull_github_comments"); break;
      case "p": app.cmd("push_github_comments"); break;
    }
  }

  window.addEventListener("keydown", handler);
  return () => window.removeEventListener("keydown", handler);
}
