import { invoke } from "@tauri-apps/api/core";
import { error as logError, warn as logWarn, info as logInfo } from "@tauri-apps/plugin-log";
import { untrack } from "svelte";
import { diffPreview } from "./diffPreview.svelte";
import { annotationDrafts } from "./annotationDrafts.svelte";
import {
  DEFAULT_COMMENT_VISIBILITY,
  type CommentVisibility,
} from "../commentVisibility";
import { DEFAULT_SYNTAX_THEME_ID } from "../syntaxThemes";
import type { AppSnapshot, SettingsTab } from "../pageTypes";
import { resolveSettingsTarget, type SettingsSectionId, type SettingsTarget } from "../settingsLink";
import { viewIdentity } from "../viewIdentity";
import { aiReviewFilter } from "./aiReviewFilter.svelte";
import { layoutPanels } from "./layoutPanels.svelte";
import { mirrorPanelToggle } from "./panelMirror";
import { pageView } from "./pageView.svelte";
import { rightRail } from "./rightRail.svelte";
import { reviewPage } from "./reviewPage.svelte";
import { ReviewPageMemory, reviewPageTabKey } from "./reviewPageModel";
import {
  HIDE_REVIEWED_KEY,
  LINE_NUMBERS_KEY,
  MARK_LINES_KEY,
  loadFlag,
  saveFlag,
} from "./viewPrefs";
import { inboxReturnView } from "../inboxNav";
import { errorText } from "./settingsAdapter";

export type { CommentVisibility };

export interface ToastMessage {
  id: number;
  kind: "success" | "error" | "info" | "warn" | "progress";
  message: string;
  /** Auto-dismiss delay this toast was shown with; absent when it persists. */
  durationMs?: number;
  /** If true, the toast will not auto-dismiss (errors always persist). */
  persist?: boolean;
  /** Optional action button shown in the toast footer. */
  action?: { label: string; onClick: () => void };
}

const TOAST_DURATION_MS: Record<ToastMessage["kind"], number> = {
  error: 10_000,
  success: 4_200,
  info: 3_200,
  warn: 3_200,
  progress: 4_200,
};

export interface LogEntry {
  ts: string;
  level: "error" | "warn" | "info";
  source: string;
  message: string;
}

const MAX_LOGS = 500;

/**
 * Dwell for a backend notification flagged `long` — the desktop's counterpart
 * to the TUI's `NOTIFICATION_DWELL_LONG`. Overrides `showToast`'s default.
 */
export const TOAST_LONG_MS = 8_000;

export type DiffViewMode = "unified" | "split";
/** `overview` and `diff` show a review page (see `reviewPage`); the rest replace it. */
export type MainViewMode =
  | "overview"
  | "diff"
  | "inbox"
  | "agent-output"
  | "export-review"
  | "settings";
const DIFF_VIEW_MODE_KEY = "er.diffViewMode";
const COMMENT_VISIBILITY_KEY = "er.commentVisibility";
const COMPACT_LINES_KEY = "er.compactLines";
const WRAP_LINES_KEY = "er.wrapLines";

function loadDiffViewMode(): DiffViewMode {
  if (typeof localStorage === "undefined") return "unified";
  const v = localStorage.getItem(DIFF_VIEW_MODE_KEY);
  return v === "split" ? "split" : "unified";
}

function loadCompactLines(): boolean {
  if (typeof localStorage === "undefined") return false;
  return localStorage.getItem(COMPACT_LINES_KEY) === "1";
}

function loadWrapLines(): boolean {
  if (typeof localStorage === "undefined") return true;
  // Default on — long lines wrap inside the fixed 50/50 panels.
  return localStorage.getItem(WRAP_LINES_KEY) !== "0";
}

function loadCommentVisibility(): CommentVisibility {
  if (typeof localStorage === "undefined") return { ...DEFAULT_COMMENT_VISIBILITY };
  try {
    return {
      ...DEFAULT_COMMENT_VISIBILITY,
      ...JSON.parse(localStorage.getItem(COMMENT_VISIBILITY_KEY) ?? "{}"),
    };
  } catch {
    return { ...DEFAULT_COMMENT_VISIBILITY };
  }
}

/**
 * Sends a command through `er_send` or another store's path, or answers null
 * to leave it to `cmd`. The stores that route hand their routes in; this
 * module does not import them.
 */
export type CmdRoute = (command: string, args: Record<string, unknown> | undefined) => Promise<void> | null;

class AppStore {
  error = $state<string | null>(null);
  toasts = $state<ToastMessage[]>([]);
  showEmptyState = $state(false);
  logs = $state<LogEntry[]>([]);
  /** Unified or side-by-side diff rendering. Persisted to localStorage. */
  diffViewMode = $state<DiffViewMode>(loadDiffViewMode());
  /** Tighter line-height in the diff view. Persisted to localStorage. */
  compactLines = $state<boolean>(loadCompactLines());
  /** Word-wrap long diff lines inside the fixed-width panels (unified and
   *  split). When off, long lines pan horizontally inside each panel.
   *  Persisted to localStorage. */
  wrapLines = $state<boolean>(loadWrapLines());
  commentVisibility = $state<CommentVisibility>(loadCommentVisibility());
  /** Drop reviewed files from the diff column. Presentation only: the engine's
   *  unreviewed filter would rebuild `files` on every checkmark, which the
   *  reviewed-revision path exists to avoid (ADR 0016). */
  hideReviewed = $state<boolean>(loadFlag(HIDE_REVIEWED_KEY, false));
  /** Line-number gutters in the diff. */
  lineNumbers = $state<boolean>(loadFlag(LINE_NUMBERS_KEY, true));
  /** Color each changed line by the commit that introduced it. */
  markLines = $state<boolean>(loadFlag(MARK_LINES_KEY, false));
  /** Shiki syntax theme id — picker UI wires here later. */
  currentSyntaxTheme = $state(DEFAULT_SYNTAX_THEME_ID);
  mainView = $state<MainViewMode>("diff");
  /** The main view the Inbox was opened from; leaving the Inbox returns there. */
  private viewBeforeInbox: MainViewMode | null = null;
  /** Where the next Settings mount should land; the page takes it and clears it. */
  settingsTarget = $state<SettingsTarget | null>(null);
  private reviewPages = new ReviewPageMemory();
  /** The review view the page showed last, so a switch resets the AI filter once. */
  private shownIdentity: string | null = null;

  private toastTimers = new Map<number, ReturnType<typeof setTimeout>>();
  private toastId = 0;

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

  toggleWrapLines() {
    this.wrapLines = !this.wrapLines;
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(WRAP_LINES_KEY, this.wrapLines ? "1" : "0");
    }
  }

  setHideReviewed(on: boolean) {
    this.hideReviewed = on;
    saveFlag(HIDE_REVIEWED_KEY, on);
  }

  setLineNumbers(on: boolean) {
    this.lineNumbers = on;
    saveFlag(LINE_NUMBERS_KEY, on);
  }

  setMarkLines(on: boolean) {
    this.markLines = on;
    saveFlag(MARK_LINES_KEY, on);
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

  showToast(
    kind: ToastMessage["kind"],
    message: string,
    durationMs = TOAST_DURATION_MS[kind],
    opts?: { persist?: boolean; action?: ToastMessage["action"] },
  ) {
    const id = ++this.toastId;
    // Errors always persist; warn persists if not explicitly overridden.
    const persist = opts?.persist ?? (kind === "error");
    this.toasts = [...this.toasts, {
      id,
      kind,
      message,
      persist,
      durationMs: persist ? undefined : durationMs,
      action: opts?.action,
    }];
    if (this.toasts.length > 4) this.toasts = this.toasts.slice(-4);
    if (!persist) {
      const timer = setTimeout(() => {
        this.toasts = this.toasts.filter((t) => t.id !== id);
        this.toastTimers.delete(id);
      }, durationMs);
      this.toastTimers.set(id, timer);
    }
  }

  /** Pause the auto-dismiss timer for a toast (called on mouseenter). */
  pauseToast(id: number) {
    const timer = this.toastTimers.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      this.toastTimers.delete(id);
    }
  }

  /** Restart the auto-dismiss timer for a toast (called on mouseleave). */
  resumeToast(id: number, durationMs: number) {
    if (this.toastTimers.has(id)) return; // already running
    const toast = this.toasts.find((t) => t.id === id);
    if (!toast || toast.persist) return;
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

  /** Paint chrome locally, then tell the backend. */
  togglePanel(panel: "left" | "tree" | "right") {
    if (panel === "right") {
      rightRail.toggle();
    } else {
      layoutPanels.toggle(panel);
    }
    void mirrorPanelToggle(panel).catch((e) => {
      console.error("togglePanel failed:", e);
      this.pushLog("error", "toggle_panel", String(e));
      this.showToast("error", `toggle_panel: ${e}`);
    });
  }

  setMainView(mode: MainViewMode) {
    if (mode === "inbox" && this.mainView !== "inbox") this.viewBeforeInbox = this.mainView;
    this.mainView = mode;
  }

  closeInbox() {
    if (this.mainView !== "inbox") return;
    this.mainView = inboxReturnView(this.viewBeforeInbox, reviewPage.available("overview"));
  }

  /** The Inbox nav rows: open it, or close it when it is already open. */
  toggleInbox() {
    if (this.mainView === "inbox") this.closeInbox();
    else this.setMainView("inbox");
  }

  /** Open Settings, optionally at a tab or a named section ("risk-rules"). */
  openSettings(to?: SettingsTab | SettingsSectionId) {
    this.settingsTarget = resolveSettingsTarget(to);
    this.mainView = "settings";
  }

  /**
   * Follows the page's view to a new tab or mode: restores the review page
   * that tab was left on, resets the AI filter, and keeps the preview and
   * draft caches to the open tabs.
   */
  followView(view: AppSnapshot | null): void {
    const next = this.reviewPages.follow(reviewPageTabKey(view), this.mainView, reviewPage.available("overview"));
    if (next) this.mainView = next;
    const identity = view && viewIdentity(view);
    if (identity !== this.shownIdentity) {
      if (this.shownIdentity !== null) aiReviewFilter.reset();
      this.shownIdentity = identity;
    }
    diffPreview.sync(view);
    annotationDrafts.sync(view);
  }

  private reportCmdError(command: string, e: unknown) {
    console.error(`${command} failed:`, e);
    const message = errorText(e);
    this.pushLog("error", command, message);
    this.error = `${command}: ${message}`;
    this.showToast("error", `${command}: ${message}`);
    setTimeout(() => {
      if (this.error?.startsWith(`${command}:`)) this.error = null;
    }, 5000);
  }

  private throwCmdErrorIfRequested(error: unknown, options: { throwOnError?: boolean }): void {
    if (options.throwOnError) throw error;
  }

  private readonly cmdRoutes: CmdRoute[] = [];

  /** Hands `cmd` a route; the first route that takes a command runs it. */
  routeCmds(route: CmdRoute): void {
    this.cmdRoutes.push(route);
  }

  private routed(command: string, args: Record<string, unknown> | undefined): Promise<void> | null {
    for (const route of this.cmdRoutes) {
      const routed = route(command, args);
      if (routed) return routed;
    }
    return null;
  }

  /**
   * Sends a command through the first route that takes it, or else invokes
   * it. What the command changed reaches the page through the resources.
   */
  async cmd(
    command: string,
    args?: Record<string, unknown>,
    options: { throwOnError?: boolean } = {},
  ): Promise<void> {
    try {
      await (this.routed(command, args) ?? invoke(command, args));
    } catch (e) {
      this.reportCmdError(command, e);
      this.throwCmdErrorIfRequested(e, options);
    }
  }
}

export const app = new AppStore();

$effect.root(() => {
  $effect(() => {
    const view = pageView.current;
    untrack(() => app.followView(view));
  });
});
