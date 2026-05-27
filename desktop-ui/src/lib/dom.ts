import { tick } from "svelte";
import { app } from "$lib/stores/app.svelte";
import { diffNav } from "$lib/stores/diffNav.svelte";
import type { FlatFinding, ThreadSnapshot } from "$lib/types";

/**
 * Scroll an element into view and pulse its outline.
 * Retained for callers that flash arbitrary IDs (not threads/findings).
 *
 * IDs follow the mock conventions:
 * - finding cards: `finding-<id>` (e.g. `finding-medium-1`)
 * - comment threads: `thread-comment-<id>` or just `thread-<id>`
 * - question threads: `thread-question-<id>`
 *
 * `behavior: "auto"` (not "smooth") — smooth scroll through long virtualized
 * lists triggers IntersectionObserver storms during the animation.
 */
export function jumpTo(id: string): void {
  const el = document.getElementById(id);
  if (!el) return;
  el.scrollIntoView({ behavior: "auto", block: "center" });
  el.classList.remove("flash");
  // Force a reflow so the animation restarts when jumping to the same element twice.
  void el.offsetWidth;
  el.classList.add("flash");
  setTimeout(() => el.classList.remove("flash"), 1300);
}

/**
 * Navigate to a thread, switching files first if needed. The Rust
 * `select_file` command takes a file index (not a path), so we look up the
 * index from the current snapshot before invoking. Engine sync runs FIRST
 * (reviewed flags, navigation cursor) before the visual scroll.
 */
export async function navigateToThread(thread: ThreadSnapshot): Promise<void> {
  const snap = app.snapshot;
  if (!snap) return;
  const currentPath = snap.files[snap.selected_file]?.path;
  if (thread.file && thread.file !== currentPath) {
    const f = snap.files.find((f) => f.path === thread.file);
    if (f) {
      await app.cmd("select_file", { idx: f.source_index });
      await tick();
    }
  }
  const didScroll = await diffNav.scrollToThread(thread.id);
  if (didScroll) return;

  const owningFinding = snap.ai?.findings.find((f) => f.thread_id === thread.id);
  if (owningFinding) {
    await diffNav.scrollToFinding(owningFinding.id, { flashId: `finding-${owningFinding.id}` });
  }
}

/**
 * Navigate to a finding, switching files first if needed.
 */
export async function navigateToFinding(finding: FlatFinding): Promise<void> {
  const snap = app.snapshot;
  if (!snap) return;
  const currentPath = snap.files[snap.selected_file]?.path;
  if (finding.file && finding.file !== currentPath) {
    const f = snap.files.find((f) => f.path === finding.file);
    if (f) {
      await app.cmd("select_file", { idx: f.source_index });
      await tick();
    }
  }
  await diffNav.scrollToFinding(finding.id, { flashId: `finding-${finding.id}` });
}
