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

/** Where an open lands: ask with the tab-open modal, or go straight to a new
 *  tab or the current one. */
export type PrOpenChoice = "ask" | "new-tab" | "replace";

/** A per-viewer UI preference, so localStorage rather than config.toml: it
 *  only changes which dialog this window shows, never what the engine does. */
const ALWAYS_OPEN_HERE_KEY = "er.alwaysOpenHere";

type PrefStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function defaultStorage(): PrefStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export function loadAlwaysOpenHere(storage: PrefStorage | null = defaultStorage()): boolean {
  try {
    return storage?.getItem(ALWAYS_OPEN_HERE_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveAlwaysOpenHere(on: boolean, storage: PrefStorage | null = defaultStorage()): void {
  try {
    if (on) storage?.setItem(ALWAYS_OPEN_HERE_KEY, "1");
    else storage?.removeItem(ALWAYS_OPEN_HERE_KEY);
  } catch {
    // Blocked storage only means the modal asks again next time.
  }
}

function wantsNewTab(e: Pick<MouseEvent, "metaKey" | "ctrlKey"> & { button?: number }): boolean {
  return e.metaKey || e.ctrlKey || e.button === 1;
}

/**
 * A plain open asks, unless the reader said "always open here", which makes it
 * replace the current tab. Cmd/Ctrl-click and middle-click always skip the
 * question and open a new tab, so remembering "here" never loses the way to a
 * new tab — the modal says so.
 */
export function resolvePrOpen(
  e: (Pick<MouseEvent, "metaKey" | "ctrlKey"> & { button?: number }) | null,
  alwaysOpenHere: boolean = loadAlwaysOpenHere(),
): PrOpenChoice {
  if (e && wantsNewTab(e)) return "new-tab";
  return alwaysOpenHere ? "replace" : "ask";
}

/**
 * For open paths that do not mount the tab-open modal (the sidebar's branch
 * rows): a plain click replaces the active tab, Cmd/Ctrl-click and
 * middle-click open a new one — what the asking paths do once the reader
 * chose "always open here".
 */
export function shouldReplaceTab(e: MouseEvent): boolean {
  return !wantsNewTab(e);
}

/** The element a keydown landed on, as far as the tab-open modal cares. */
export interface KeyTarget {
  tagName: string;
  hasAttribute(name: string): boolean;
}

/**
 * What Enter means inside the tab-open modal. Cmd/Ctrl+Enter replaces from
 * anywhere. A bare Enter takes the default (new tab) only from the panel or
 * the primary button; on any other button it is that button's own press, so
 * null leaves the click to the browser.
 */
export function tabOpenKeyChoice(
  e: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey">,
  target: KeyTarget | null,
): "new-tab" | "replace" | null {
  if (e.key !== "Enter") return null;
  if (target?.tagName === "INPUT") return null;
  if (e.metaKey || e.ctrlKey) return "replace";
  if (target?.tagName === "BUTTON" && !target.hasAttribute("data-tm-primary")) return null;
  return "new-tab";
}

/** What the tab-open modal shows about the thing being opened. */
export interface TabOpenRequest {
  /** Identity of this open; the modal resets its choices only when it changes. */
  key: string;
  title: string;
  /** "repo #123", in mono. */
  ref: string;
  /** "Comment on your PR · 21m ago"; empty when the open has no triggering event. */
  context: string;
  iconPath: string;
  /** A theme colour token name (e.g. "comment"), never a raw colour. */
  tone: string;
  /** The last comment, quoted so the reader knows what they are jumping to. */
  quote: string | null;
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
