import { closeAiActionPalette } from "$lib/components/AiActionPalette.svelte";
import { invoke } from "@tauri-apps/api/core";
import { error as logError, warn as logWarn, info as logInfo } from "@tauri-apps/plugin-log";
import { tick } from "svelte";
import type { AppSnapshot, PollResponse } from "../types";

export interface ToastMessage {
  id: number;
  kind: "success" | "error" | "info";
  message: string;
}

export interface LogEntry {
  ts: string;
  level: "error" | "warn" | "info";
  source: string;
  message: string;
}

const MAX_LOGS = 500;

export type DiffViewMode = "unified" | "split";
export type MainViewMode = "diff" | "agent-output" | "export-review";
const DIFF_VIEW_MODE_KEY = "er.diffViewMode";
const COMMENT_VISIBILITY_KEY = "er.commentVisibility";
const COMPACT_LINES_KEY = "er.compactLines";

export interface CommentVisibility {
  hideAll: boolean;
  hideResolved: boolean;
  hideOutdated: boolean;
}

function loadDiffViewMode(): DiffViewMode {
  if (typeof localStorage === "undefined") return "unified";
  const v = localStorage.getItem(DIFF_VIEW_MODE_KEY);
  return v === "split" ? "split" : "unified";
}

function loadCompactLines(): boolean {
  if (typeof localStorage === "undefined") return false;
  return localStorage.getItem(COMPACT_LINES_KEY) === "1";
}

function loadCommentVisibility(): CommentVisibility {
  const defaults = { hideAll: false, hideResolved: true, hideOutdated: true };
  if (typeof localStorage === "undefined") return defaults;
  try {
    return { ...defaults, ...JSON.parse(localStorage.getItem(COMMENT_VISIBILITY_KEY) ?? "{}") };
  } catch {
    return defaults;
  }
}

/** Commands that typically take 2-3s to complete on the backend. */
const SLOW_COMMANDS = new Set([
  "open_local_branch",
  "open_pr_review",
  "select_tab",
  "set_mode",
  "open_remote_pr",
  "open_pr_branch",
]);

function timingSegmentMs(start: number, end: number): number {
  return Math.max(0, Math.round(end - start));
}

function nextAnimationFrame(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => resolve());
      return;
    }
    setTimeout(resolve, 0);
  });
}

class AppStore {
  snapshot = $state<AppSnapshot | null>(null);
  loading = $state(false);
  /** True while a slow tab-switch or branch-open command is in flight. */
  switching = $state(false);
  /** User-facing label for the active slow command. */
  switchingLabel = $state<string | null>(null);
  /** True while force_refresh_diff is fetching from the remote. */
  refreshing = $state(false);
  error = $state<string | null>(null);
  toasts = $state<ToastMessage[]>([]);
  showEmptyState = $state(false);
  logs = $state<LogEntry[]>([]);
  /** Unified or side-by-side diff rendering. Persisted to localStorage. */
  diffViewMode = $state<DiffViewMode>(loadDiffViewMode());
  /** Tighter line-height in the diff view. Persisted to localStorage. */
  compactLines = $state<boolean>(loadCompactLines());
  commentVisibility = $state<CommentVisibility>(loadCommentVisibility());
  mainView = $state<MainViewMode>("diff");

  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  private toastTimers = new Map<number, ReturnType<typeof setTimeout>>();
  private toastId = 0;
  private lastSnapshotNotification: string | null = null;
  private pollIntervalMs = 2000;
  private lastPollRevision: number | null = null;

  /** Cycle unified → split → unified. Persists the choice. */
  toggleDiffViewMode() {
    this.diffViewMode = this.diffViewMode === "unified" ? "split" : "unified";
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(DIFF_VIEW_MODE_KEY, this.diffViewMode);
    }
  }

  setDiffViewMode(mode: DiffViewMode) {
    if (this.diffViewMode === mode) return;
    this.diffViewMode = mode;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(DIFF_VIEW_MODE_KEY, mode);
    }
  }

  toggleCompactLines() {
    this.compactLines = !this.compactLines;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(COMPACT_LINES_KEY, this.compactLines ? "1" : "0");
    }
  }

  setCommentVisibility(next: Partial<CommentVisibility>) {
    this.commentVisibility = { ...this.commentVisibility, ...next };
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(COMMENT_VISIBILITY_KEY, JSON.stringify(this.commentVisibility));
    }
  }

  pushLog(level: LogEntry["level"], source: string, message: string) {
    this.logs.push({ ts: new Date().toISOString(), level, source, message });
    if (this.logs.length > MAX_LOGS) this.logs.splice(0, this.logs.length - MAX_LOGS);
    const line = `[${source}] ${message}`;
    if (level === "error") logError(line).catch(() => {});
    else if (level === "warn") logWarn(line).catch(() => {});
    else logInfo(line).catch(() => {});
  }

  clearLogs() {
    this.logs = [];
  }

  dumpLogs(): string {
    return this.logs
      .map((l) => `[${l.ts}] ${l.level.toUpperCase()} ${l.source}: ${l.message}`)
      .join("\n");
  }

  showToast(kind: ToastMessage["kind"], message: string, durationMs = kind === "error" ? 10_000 : 2_000) {
    const id = ++this.toastId;
    this.toasts = [...this.toasts, { id, kind, message }];
    if (this.toasts.length > 4) this.toasts = this.toasts.slice(-4);
    const timer = setTimeout(() => {
      this.toasts = this.toasts.filter((t) => t.id !== id);
      this.toastTimers.delete(id);
    }, durationMs);
    this.toastTimers.set(id, timer);
  }

  closeToast(id?: number) {
    if (id !== undefined) {
      const timer = this.toastTimers.get(id);
      if (timer !== undefined) clearTimeout(timer);
      this.toastTimers.delete(id);
      this.toasts = this.toasts.filter((t) => t.id !== id);
    } else {
      for (const timer of this.toastTimers.values()) clearTimeout(timer);
      this.toastTimers.clear();
      this.toasts = [];
    }
  }

  private syncSnapshotToast(snapshot: AppSnapshot | null): boolean {
    const message = snapshot?.notification ?? null;
    if (message === null) {
      this.lastSnapshotNotification = null;
      return false;
    }
    if (message === this.lastSnapshotNotification) return false;

    this.lastSnapshotNotification = message;
    const lower = message.toLowerCase();
    const kind: "success" | "error" =
      lower.includes("failed") ||
      lower.includes("error") ||
      lower.includes("invalid") ||
      lower.includes("not found") ||
      lower.includes("cannot") ||
      lower.includes("no ")
        ? "error"
        : "success";
    this.showToast(kind, message);
    return true;
  }

  async load() {
    this.loading = true;
    try {
      this.snapshot = await invoke<AppSnapshot>("get_snapshot");
      this.syncSnapshotToast(this.snapshot);
      this.lastPollRevision = null;
    } catch (e) {
      this.error = String(e);
      this.showToast("error", String(e));
    } finally {
      this.loading = false;
    }
  }

  startPolling(intervalMs = 2000) {
    if (this.pollTimer !== null) return;
    this.pollIntervalMs = intervalMs;
    const tick = async () => {
      if (this.pollTimer === null) return;
      try {
        const next = await invoke<PollResponse>("poll");
        if (this.lastPollRevision !== next.revision) {
          this.lastPollRevision = next.revision;
          // snapshot is null when only the revision changed on the backend
          // but no full snapshot was built (unchanged poll optimization).
          // That shouldn't happen since revision != lastPollRevision implies
          // the backend built a snapshot, but guard defensively.
          if (next.snapshot !== null) {
            this.snapshot = next.snapshot;
            this.syncSnapshotToast(this.snapshot);
          }
        }
      } catch {
        // Silently ignore poll errors (window may be closing)
      }
      if (this.pollTimer !== null) {
        this.pollTimer = setTimeout(tick, this.pollIntervalMs);
      }
    };
    this.pollTimer = setTimeout(tick, this.pollIntervalMs);
  }

  stopPolling() {
    if (this.pollTimer !== null) {
      clearTimeout(this.pollTimer);
      this.pollTimer = null;
    }
  }

  async togglePanel(panel: "left" | "tree" | "right") {
    try {
      this.snapshot = await invoke<AppSnapshot>("toggle_panel", { panel });
      this.syncSnapshotToast(this.snapshot);
    } catch (e) {
      console.error("togglePanel failed:", e);
      this.pushLog("error", "toggle_panel", String(e));
      this.showToast("error", `toggle_panel: ${e}`);
    }
  }

  setMainView(mode: MainViewMode) {
    this.mainView = mode;
  }

  async cmd(command: string, args?: Record<string, unknown>) {
    if (command === "run_ai_review" || command === "run_ai_validate") {
      closeAiActionPalette();
    }
    const tStart = performance.now();
    const isSlow = SLOW_COMMANDS.has(command);
    const isForceRefresh = command === "force_refresh_diff";
    const tSwitchingSet = performance.now();
    if (isSlow) {
      this.switching = true;
      this.switchingLabel = switchingLabelForCommand(command);
    }
    if (isForceRefresh) this.refreshing = true;
    if (isSlow || isForceRefresh) {
      await tick();
      await nextAnimationFrame();
    }
    const tInvokeStart = performance.now();
    try {
      this.snapshot = await invoke<AppSnapshot>(command, args);
      const tInvokeDone = performance.now();
      const hadBackendToast = this.syncSnapshotToast(this.snapshot);
      if (!hadBackendToast) {
        const message = successToastForCommand(command);
        if (message) this.showToast("success", message);
      }
      this.lastPollRevision = null;
      const tSnapshotApplied = performance.now();
      const totalMs = timingSegmentMs(tStart, tSnapshotApplied);
      if (totalMs > 500) {
        const paintMs = timingSegmentMs(tSwitchingSet, tInvokeStart);
        const invokeMs = timingSegmentMs(tInvokeStart, tInvokeDone);
        logWarn(
          `cmd_timing command=${command} paint_ms=${paintMs} invoke_ms=${invokeMs} total_ms=${totalMs} snapshot_ms=${timingSegmentMs(tInvokeDone, tSnapshotApplied)}`
        ).catch(() => {});
      }
    } catch (e) {
      console.error(`${command} failed:`, e);
      this.pushLog("error", command, String(e));
      this.error = `${command}: ${e}`;
      this.showToast("error", `${command}: ${e}`);
      setTimeout(() => {
        if (this.error?.startsWith(`${command}:`)) this.error = null;
      }, 5000);
    } finally {
      if (isSlow) {
        this.switching = false;
        this.switchingLabel = null;
      }
      if (isForceRefresh) this.refreshing = false;
    }
  }
}

export const app = new AppStore();

function successToastForCommand(command: string): string | null {
  switch (command) {
    case "force_refresh_diff":
      return "Diff force refreshed";
    case "refresh_github_status":
      return "GitHub status refreshed";
    case "run_ai_review":
      return "AI review started";
    default:
      return null;
  }
}

function switchingLabelForCommand(command: string): string {
  switch (command) {
    case "open_local_branch":
      return "Opening branch...";
    case "open_pr_review":
    case "open_remote_pr":
    case "open_pr_branch":
      return "Opening PR...";
    case "select_tab":
      return "Switching tab...";
    case "set_mode":
      return "Switching mode...";
    default:
      return "Switching review...";
  }
}
