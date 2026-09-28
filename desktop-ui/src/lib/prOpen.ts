import { invoke } from "@tauri-apps/api/core";
import { tick } from "svelte";
import type { PrInfo } from "$lib/types";

export interface PrOpenHint {
  baseRef: string;
  headRef: string;
  headOid: string;
  updatedAt: string;
  title: string;
  author: string;
}

/** Plain click replaces the active tab. Cmd/Ctrl-click or middle-click opens
 * a new tab. (Inverse of the previous behavior — power users use modifiers
 * when they want to keep the current tab around.) */
export function shouldReplaceTab(e: MouseEvent): boolean {
  return !(e.metaKey || e.ctrlKey || e.button === 1);
}

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => resolve());
    } else {
      setTimeout(resolve, 0);
    }
  });
}

export async function yieldForPendingPaint() {
  await tick();
  await nextAnimationFrame();
}

export function remoteParts(project: { remote?: string | null }): { owner: string; repo: string } | null {
  const remote = project.remote?.trim();
  if (!remote) return null;
  const withoutScheme = remote
    .replace(/^https?:\/\/github\.com\//, "")
    .replace(/\.git$/, "")
    .replace(/^\/+|\/+$/g, "");
  const [owner, repo] = withoutScheme.split("/");
  if (!owner || !repo) return null;
  return { owner, repo };
}

export function buildPrHint(pr: PrInfo): PrOpenHint | undefined {
  if (!pr.base_ref?.trim() || !pr.head_ref?.trim() || !pr.head_oid?.trim()) {
    return undefined;
  }
  return {
    baseRef: pr.base_ref,
    headRef: pr.head_ref,
    headOid: pr.head_oid,
    updatedAt: pr.updated_at,
    title: pr.title,
    author: pr.author,
  };
}

// ── PR hover-prefetch ──
// After a short debounce on hover, kick a background `prefetch_pr_open` to
// warm the diff cache so the click feels instant. If the cursor leaves
// before the debounce fires, the timer is cleared and no fetch starts.
const PR_HOVER_PREFETCH_DELAY_MS = 150;

type InvokeFn = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

/** One debounce-timer set per sidebar instance. `invokeFn` and `delayMs` are
 *  the test seam. */
export function createPrPrefetch(invokeFn: InvokeFn = invoke, delayMs = PR_HOVER_PREFETCH_DELAY_MS) {
  const prPrefetchTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function schedulePrPrefetch(projectId: string, pr: PrInfo) {
    // No useful hint to send → skip; the open path falls back to the slow
    // synchronous gh-pr-view round-trip anyway.
    if (!pr.head_oid || !pr.base_ref) return;
    const key = `${projectId}:${pr.number}`;
    if (prPrefetchTimers.has(key)) return;
    const timer = setTimeout(() => {
      prPrefetchTimers.delete(key);
      // Bypass app.cmd() — that assigns the return value to app.snapshot, and
      // prefetch_pr_open returns () which would null out the snapshot and
      // render the empty page. Fire-and-forget invoke is correct here.
      invokeFn("prefetch_pr_open", {
        projectId,
        prNumber: pr.number,
        hint: buildPrHint(pr),
      }).catch(() => {
        // Background fetch — failure is logged in Rust, nothing to do here.
      });
    }, delayMs);
    prPrefetchTimers.set(key, timer);
  }

  /** Remote-only projects have no local clone — the open path would be three
   *  synchronous `gh` calls. Warm the remote PR open cache on hover so the
   *  click opens with zero network. Same debounce/dedupe/cancel discipline. */
  function scheduleRemotePrPrefetch(project: { id: string; remote?: string | null }, pr: PrInfo) {
    const parts = remoteParts(project);
    if (!parts) return;
    const key = `remote:${project.id}:${pr.number}`;
    if (prPrefetchTimers.has(key)) return;
    const timer = setTimeout(() => {
      prPrefetchTimers.delete(key);
      invokeFn("prefetch_remote_pr_open", {
        owner: parts.owner,
        repo: parts.repo,
        number: pr.number,
      }).catch(() => {
        // Background fetch — failure is logged in Rust, nothing to do here.
      });
    }, delayMs);
    prPrefetchTimers.set(key, timer);
  }

  function cancelPrPrefetch(projectId: string, prNumber: number) {
    for (const key of [`${projectId}:${prNumber}`, `remote:${projectId}:${prNumber}`]) {
      const timer = prPrefetchTimers.get(key);
      if (timer !== undefined) {
        clearTimeout(timer);
        prPrefetchTimers.delete(key);
      }
    }
  }

  return { schedulePrPrefetch, scheduleRemotePrPrefetch, cancelPrPrefetch };
}
