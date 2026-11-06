import { tick } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import { app } from "$lib/stores/app.svelte";
import { diffNav } from "$lib/stores/diffNav.svelte";
import { reviewPage } from "$lib/stores/reviewPage.svelte";

/** ⌘-click (Ctrl-click off macOS) asks for the source rather than the diff,
 *  as the branch bar's links did before the top bar replaced it. */
export function wantsSource(e: MouseEvent | KeyboardEvent | undefined): boolean {
  return !!e && (e.metaKey || e.ctrlKey);
}

async function openSource(idx: number): Promise<void> {
  await app.cmd("select_file", { idx });
  try {
    const res = await invoke<{ kind: string; target: string }>("open_source");
    if (res.kind === "needs_checkout") app.showToast("info", "Create editable worktree to open locally");
  } catch (err) {
    app.showToast("error", `Open source: ${err}`);
  }
}

/**
 * A file named on the Overview, where no diff is mounted: a plain click shows
 * the diff at that file, ⌘-click opens the file itself (the local checkout in
 * the editor, else the file on GitHub) and leaves the Overview where it is.
 */
export async function openOverviewFile(path: string, e?: MouseEvent): Promise<void> {
  const file = app.snapshot?.files.find((f) => f.path === path);
  if (!file) return;
  if (wantsSource(e)) return openSource(file.source_index);
  reviewPage.show("diff");
  await tick();
  await app.cmd("select_file", { idx: file.source_index });
  await tick();
  await diffNav.scrollToFile(path);
}
