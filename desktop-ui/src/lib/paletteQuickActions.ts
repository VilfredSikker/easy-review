// Branch, worktree and PR-link actions that used to live as icon buttons in
// the branch context bar. The redesigned top bar has no slot for them, so the
// palette carries them and keeps them reachable by key.

import { invoke } from "@tauri-apps/api/core";
import { copyToClipboard } from "$lib/clipboard";
import { openExternalUrl } from "$lib/openExternalUrl";
import { buildPrHint, remoteParts } from "$lib/prOpen";
import { app } from "$lib/stores/app.svelte";
import type { PrInfo, ProjectSnapshot } from "$lib/types";

async function copyWithToast(text: string, message: string): Promise<void> {
  try {
    await copyToClipboard(text);
    app.showToast("success", message);
  } catch (e) {
    app.showToast("error", `Copy failed: ${e}`);
  }
}

export function copyBranchName(branch: string): Promise<void> {
  return copyWithToast(branch, "Branch name copied");
}

export function copyWorktreePath(path: string): Promise<void> {
  return copyWithToast(path, "Worktree path copied");
}

export async function revealWorktree(path: string): Promise<void> {
  try {
    await invoke("reveal_path", { path });
    app.showToast("success", "Revealed in Finder");
  } catch (e) {
    app.showToast("error", `Reveal failed: ${e}`);
  }
}

export function copyPrLink(url: string): Promise<void> {
  return copyWithToast(url, "Copied PR link");
}

export function openPrLink(url: string): Promise<void> {
  return openExternalUrl(url);
}

/**
 * Opens a listed PR in a new tab, the way a modified click in the sidebar
 * does. The hint pins the head the list saw (ADR 0040); a remote-only project
 * names its repo explicitly because there is no local clone to resolve it.
 */
export async function openListedPr(project: ProjectSnapshot, pr: PrInfo): Promise<void> {
  if (project.remote_only) {
    const parts = remoteParts(project);
    if (!parts) return;
    await app.cmd("open_remote_pr", { ...parts, number: pr.number, replace: false });
    return;
  }
  await app.cmd("open_pr_review", {
    projectId: project.id,
    prNumber: pr.number,
    replace: false,
    hint: buildPrHint(pr),
  });
}
