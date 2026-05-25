import { closeAiActionPalette } from "$lib/components/AiActionPalette.svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { error as logError, warn as logWarn, info as logInfo } from "@tauri-apps/plugin-log";
import { tick } from "svelte";
import { profileLog } from "../profileLog";
import { DEFAULT_SYNTAX_THEME_ID } from "../syntaxThemes";
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

/** Patch chrome/sidebar fields from a poll snapshot while keeping diff hunks/spans. */
function mergeChromeSnapshot(prev: AppSnapshot, next: AppSnapshot): AppSnapshot {
  return {
    ...next,
    mode: prev.mode,
    branch: prev.branch,
    base: prev.base,
    input_mode: prev.input_mode,
    files: prev.files,
    selected_file: prev.selected_file,
    current_hunk: prev.current_hunk,
    filter: prev.filter,
    reviewed_count: prev.reviewed_count,
    total_count: prev.total_count,
    ai: prev.ai,
    pr: prev.pr,
    ui_annotations: prev.ui_annotations,
    browser: prev.browser,
    filter_suggestions: prev.filter_suggestions,
    commits: prev.commits,
    selected_commit_sha: prev.selected_commit_sha,
  };
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
  /** Shiki syntax theme id — picker UI wires here later. */
  currentSyntaxTheme = $state(DEFAULT_SYNTAX_THEME_ID);
  mainView = $state<MainViewMode>("diff");

  private pollTimer: ReturnType<typeof setTimeout> | null = null;
  private toastTimers = new Map<number, ReturnType<typeof setTimeout>>();
  private toastId = 0;
  private lastSnapshotNotification: string | null = null;
  // Safety-net interval — the backend pushes a `er://revision` event on every
  // state change, so this is just a fallback in case an event is dropped or
  // the listener hasn't attached yet. Used to be 2s when polling was the
  // primary mechanism.
  private pollIntervalMs = 30_000;
  private lastPollRevision: number | null = null;
  private lastPollContentRevision: number | null = null;
  private lastPollChromeRevision: number | null = null;
  private revisionUnlisten: UnlistenFn | null = null;
  private pollInFlight = false;

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
      this.lastPollContentRevision = null;
      this.lastPollChromeRevision = null;
    } catch (e) {
      this.error = String(e);
      this.showToast("error", String(e));
    } finally {
      this.loading = false;
    }
  }

  /**
   * Push model: the backend emits `er://revision` whenever its desktop_revision
   * counter advances. We respond by calling `poll` to fetch the updated
   * snapshot. A long-interval safety timer covers the rare case where an event
   * is missed (window mid-resume, dropped from queue, etc.).
   *
   * Coalesce concurrent calls with `pollInFlight` so a burst of events at
   * startup doesn't spam the backend mutex.
   */
  startPolling(intervalMs = 30_000) {
    if (this.pollTimer !== null) return;
    this.pollIntervalMs = intervalMs;

    const doPoll = async (trigger: "revision_event" | "safety_timer" | "unknown" = "unknown") => {
      if (this.pollInFlight) return;
      this.pollInFlight = true;
      const t0 = performance.now();
      profileLog("poll_invoke_start", { trigger });
      try {
        const next = await invoke<PollResponse>("poll");
        const invokeMs = Math.round(performance.now() - t0);
        const hadSnapshot = next.snapshot !== null;
        const contentChanged =
          this.lastPollContentRevision !== next.content_revision;
        const chromeChanged =
          this.lastPollChromeRevision !== next.chrome_revision;
        if (contentChanged || chromeChanged) {
          this.lastPollRevision = next.revision;
          this.lastPollContentRevision = next.content_revision;
          this.lastPollChromeRevision = next.chrome_revision;
          if (next.snapshot !== null) {
            const useMerge =
              this.snapshot !== null &&
              (next.chrome_only || !contentChanged);
            if (useMerge) {
              this.snapshot = mergeChromeSnapshot(this.snapshot!, next.snapshot);
              profileLog("snapshot_chrome_merge", {
                invoke_ms: invokeMs,
                revision: next.revision,
                chrome_only: next.chrome_only ? 1 : 0,
                trigger,
              });
            } else {
              this.snapshot = next.snapshot;
              profileLog("snapshot_replace", {
                invoke_ms: invokeMs,
                revision: next.revision,
                files: next.snapshot.files.length,
                trigger,
              });
            }
            this.syncSnapshotToast(this.snapshot);
          }
        }
        profileLog("poll_invoke_done", {
          invoke_ms: invokeMs,
          revision: next.revision,
          had_snapshot: hadSnapshot ? 1 : 0,
          trigger,
        });
      } catch {
        // Silently ignore poll errors (window may be closing).
      } finally {
        this.pollInFlight = false;
      }
    };

    // Event-driven: react to backend revision bumps. listen() is async but
    // we don't await — events that fire before the listener attaches are
    // safely covered by the safety-net interval and the initial load().
    listen<number>("er://revision", (event) => {
      profileLog("revision_event", { coalesced_rev: event.payload });
      void doPoll("revision_event");
    }).then((unlisten) => {
      this.revisionUnlisten = unlisten;
    });

    // Safety-net poll — long interval; only fires if events were missed.
    const tick = async () => {
      if (this.pollTimer === null) return;
      await doPoll("safety_timer");
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
    if (this.revisionUnlisten !== null) {
      this.revisionUnlisten();
      this.revisionUnlisten = null;
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
    if (
      command === "run_ai_review"
      || command === "run_ai_expert_review"
      || command === "run_ai_professor_review"
      || command === "run_ai_scoped_review"
      || command === "validate_with_ai"
      || command === "run_ai_validate"
      || command === "run_ai_review_files"
    ) {
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
      this.lastPollContentRevision = null;
      this.lastPollChromeRevision = null;
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
    case "run_ai_review_files":
      return null; // backend notify includes file count + review-files.txt hint
    case "run_ai_expert_review":
      return "Specialized review started";
    case "run_ai_professor_review":
    case "run_ai_scoped_review":
      return null;
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
