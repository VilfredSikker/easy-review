import { invoke } from "@tauri-apps/api/core";
import { error as logError, warn as logWarn, info as logInfo } from "@tauri-apps/plugin-log";
import type { AppSnapshot, PollResponse } from "../types";

export interface ToastMessage {
  id: number;
  kind: "success" | "error";
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
const DIFF_VIEW_MODE_KEY = "er.diffViewMode";
const COMMENT_VISIBILITY_KEY = "er.commentVisibility";

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

function loadCommentVisibility(): CommentVisibility {
  const defaults = { hideAll: false, hideResolved: true, hideOutdated: true };
  if (typeof localStorage === "undefined") return defaults;
  try {
    return { ...defaults, ...JSON.parse(localStorage.getItem(COMMENT_VISIBILITY_KEY) ?? "{}") };
  } catch {
    return defaults;
  }
}

class AppStore {
  snapshot = $state<AppSnapshot | null>(null);
  loading = $state(false);
  error = $state<string | null>(null);
  toast = $state<ToastMessage | null>(null);
  showEmptyState = $state(false);
  logs = $state<LogEntry[]>([]);
  /** Unified or side-by-side diff rendering. Persisted to localStorage. */
  diffViewMode = $state<DiffViewMode>(loadDiffViewMode());
  commentVisibility = $state<CommentVisibility>(loadCommentVisibility());

  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  private toastTimer: ReturnType<typeof setTimeout> | null = null;
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

  showToast(kind: ToastMessage["kind"], message: string, durationMs = kind === "error" ? 10_000 : 5_000) {
    if (this.toastTimer !== null) {
      clearTimeout(this.toastTimer);
      this.toastTimer = null;
    }
    this.toast = { id: ++this.toastId, kind, message };
    this.toastTimer = setTimeout(() => {
      if (this.toast?.id === this.toastId) this.toast = null;
      this.toastTimer = null;
    }, durationMs);
  }

  closeToast() {
    if (this.toastTimer !== null) {
      clearTimeout(this.toastTimer);
      this.toastTimer = null;
    }
    this.toast = null;
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
    const kind: ToastMessage["kind"] =
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
          this.snapshot = next.snapshot;
          this.syncSnapshotToast(this.snapshot);
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

  async cmd(command: string, args?: Record<string, unknown>) {
    const t = performance.now();
    try {
      this.snapshot = await invoke<AppSnapshot>(command, args);
      const hadBackendToast = this.syncSnapshotToast(this.snapshot);
      if (!hadBackendToast) {
        const message = successToastForCommand(command);
        if (message) this.showToast("success", message);
      }
      this.lastPollRevision = null;
      const ms = Math.round(performance.now() - t);
      if (ms > 500) logWarn(`[cmd] ${command} took ${ms}ms`).catch(() => {});
    } catch (e) {
      console.error(`${command} failed:`, e);
      this.pushLog("error", command, String(e));
      this.error = `${command}: ${e}`;
      this.showToast("error", `${command}: ${e}`);
      setTimeout(() => {
        if (this.error?.startsWith(`${command}:`)) this.error = null;
      }, 5000);
    }
  }
}

export const app = new AppStore();

function successToastForCommand(command: string): string | null {
  switch (command) {
    case "refresh_github_status":
      return "GitHub status refreshed";
    case "run_ai_review":
      return "AI review started";
    default:
      return null;
  }
}
