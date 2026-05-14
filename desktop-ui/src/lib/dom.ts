import { tick } from "svelte";
import { app } from "$lib/stores/app.svelte";
import type { ThreadSnapshot } from "$lib/types";

/**
 * Smooth-scroll an element into view and pulse its outline.
 * Used to navigate from list-style references (e.g. AI Review tile, Comments
 * card thread row) to the inline block that lives inside the diff.
 *
 * IDs follow the mock conventions:
 * - finding cards: `finding-<id>` (e.g. `finding-medium-1`)
 * - comment threads: `thread-comment-<id>` or just `thread-<id>`
 * - question threads: `thread-question-<id>`
 */
export function jumpTo(id: string): void {
  const el = document.getElementById(id);
  if (!el) return;
  el.scrollIntoView({ behavior: "smooth", block: "center" });
  el.classList.remove("flash");
  // Force a reflow so the animation restarts when jumping to the same element twice.
  void el.offsetWidth;
  el.classList.add("flash");
  setTimeout(() => el.classList.remove("flash"), 1300);
}

/** Alias used by callers that want to be explicit about the scroll+flash behavior. */
export const scrollFlash = jumpTo;

/**
 * Navigate to a thread, switching files first if needed. The Rust
 * `select_file` command takes a file index (not a path), so we look up the
 * index from the current snapshot before invoking.
 */
export async function navigateToThread(thread: ThreadSnapshot): Promise<void> {
  const snap = app.snapshot;
  if (!snap) return;
  const currentPath = snap.files[snap.selected_file]?.path;
  if (thread.file && thread.file !== currentPath) {
    const idx = snap.files.findIndex((f) => f.path === thread.file);
    if (idx >= 0) {
      await app.cmd("select_file", { idx });
      await tick();
    }
  }
  jumpTo(thread.id);
}
