import { invoke } from "@tauri-apps/api/core";
import { arenaError, arenaLog, arenaWarn } from "$lib/arena/log";
import { normalizeArenaSnapshot } from "$lib/arena/normalize";
import {
  agentLabelFromSnapshot,
  isArenaRunFromSnapshot,
  isSingleReviewRun,
} from "$lib/arena/runKind";
import { app } from "$lib/stores/app.svelte";
import { aiReviewFilter } from "$lib/stores/aiReviewFilter.svelte";
import type {
  ArenaProgressState,
  ArenaRunSnapshot,
  ArenaRunSummary,
  ArenaScope,
  ReviewerRef,
} from "$lib/types/arena";

export type ArenaLauncherMode = "arena" | "single";
export type LauncherReviewerMode = "models" | "agents";

export interface AgentGroupConfig {
  agent_kind: string;
  models: ReviewerRef[];
  title?: string;
}

export interface ArenaStartConfig {
  title?: string;
  mode?: LauncherReviewerMode;
  reviewers?: ReviewerRef[];
  agent_groups?: AgentGroupConfig[];
  scope: ArenaScope;
  rounds?: number;
  arbiter?: ReviewerRef;
  files?: string[];
  confirm?: boolean;
  /** Per-run effort override; omit to use global session effort. */
  effort?: string;
}

export interface LiveRunEntry {
  runId: string;
  agentKind?: string;
  title?: string;
}

export interface LiveRunState {
  snapshot: ArenaRunSnapshot | null;
  progress: ArenaProgressState | null;
}

export function isArenaRunActive(
  status: ArenaRunSummary["status"] | ArenaRunSnapshot["run"]["status"],
): boolean {
  if (status === "queued") return true;
  if (typeof status === "object" && status !== null && "running" in status) return true;
  return false;
}

export {
  isArenaRunFromSnapshot,
  isSingleReviewRun,
} from "$lib/arena/runKind";

class ArenaStore {
  launcherMode = $state<ArenaLauncherMode>("arena");
  launcherOpen = $state(false);
  runningOpen = $state(false);
  runningMinimized = $state(false);
  overlayOpen = $state(false);
  /** Live run tracked for progress modal + pill. */
  liveRunId = $state<string | null>(null);
  liveRuns = $state<LiveRunEntry[]>([]);
  /** Per-run poll state when batching multiple agent arenas. */
  liveRunStates = $state<Record<string, LiveRunState>>({});
  /** Run shown in results overlay (may be historical). */
  overlayRunId = $state<string | null>(null);
  lastConfig = $state<ArenaStartConfig | null>(null);
  liveSnapshot = $state<ArenaRunSnapshot | null>(null);
  overlaySnapshot = $state<ArenaRunSnapshot | null>(null);
  progress = $state<ArenaProgressState | null>(null);
  layoutMode = $state<"bracket" | "matrix" | "funnel">("bracket");
  loading = $state(false);
  runStartedAt = $state<number | null>(null);

  /** @deprecated use liveSnapshot — kept for components not yet migrated */
  get snapshot(): ArenaRunSnapshot | null {
    return this.overlayOpen ? this.overlaySnapshot : this.liveSnapshot;
  }

  get activeRunId(): string | null {
    return this.liveRunId;
  }

  get enabled(): boolean {
    return app.snapshot?.arena_enabled ?? true;
  }

  get summaries(): ArenaRunSummary[] {
    return app.snapshot?.arena_runs ?? [];
  }

  /** Runs on the current branch (snapshot list is already branch-filtered; keep client filter as guard). */
  get branchSummaries(): ArenaRunSummary[] {
    const branch = app.snapshot?.branch;
    if (!branch) return this.summaries;
    return this.summaries.filter((s) => s.branch_ref === branch);
  }

  get hasLiveRun(): boolean {
    const id = app.snapshot?.active_arena_run ?? this.liveRunId;
    if (!id) return false;
    const sum = this.summaries.find((s) => s.id === id);
    return sum ? isArenaRunActive(sum.status) : this.runningOpen;
  }

  openLauncher(preset?: ReviewerRef[], mode: ArenaLauncherMode = "arena") {
    arenaLog("store: openLauncher", {
      enabled: this.enabled,
      preset: preset?.length ?? 0,
      mode,
    });
    if (!this.enabled) {
      arenaWarn("store: openLauncher blocked — arena disabled");
      return;
    }
    this.launcherMode = mode;
    if (preset?.length) {
      this.lastConfig = {
        reviewers: preset,
        scope: scopeFromMode(app.snapshot?.mode),
        rounds: mode === "single" || preset.length < 2 ? 1 : 3,
      };
    }
    this.launcherOpen = true;
  }

  openSingleReviewLauncher() {
    this.openLauncher(undefined, "single");
  }

  closeLauncher() {
    this.launcherOpen = false;
  }

  async startRun(config: ArenaStartConfig) {
    arenaLog("store: startRun", config);
    if (!this.enabled) {
      arenaWarn("store: startRun blocked — arena disabled");
      app.showToast("error", "AI Review Arena is disabled in settings");
      return;
    }
    this.loading = true;
    this.lastConfig = config;
    this.liveRunStates = {};
    try {
      if (config.mode === "agents" && config.agent_groups?.length) {
        const runIds = await invoke<string[]>("arena_start_batch", {
          req: {
            scope: config.scope,
            files: config.files,
            rounds: config.rounds != null ? Number(config.rounds) : undefined,
            arbiter: config.arbiter,
            confirm: config.confirm ?? false,
            effort: config.effort,
            groups: config.agent_groups.map((g) => ({
              agent_kind: g.agent_kind,
              models: g.models,
              title: g.title,
            })),
          },
        });
        this.liveRuns = runIds.map((runId, i) => ({
          runId,
          agentKind: config.agent_groups![i]?.agent_kind,
          title: config.agent_groups![i]?.title,
        }));
        this.liveRunId = runIds[0] ?? null;
      } else {
        const reviewers = config.reviewers ?? [];
        const runId = await invoke<string>("arena_start", {
          req: {
            title: config.title,
            reviewers,
            scope: config.scope,
            files: config.files,
            rounds: config.rounds != null ? Number(config.rounds) : undefined,
            arbiter: config.arbiter,
            confirm: config.confirm ?? false,
            agent_kind: undefined,
            effort: config.effort,
          },
        });
        this.liveRuns = [{ runId, title: config.title }];
        this.liveRunId = runId;
      }
      this.runStartedAt = Date.now();
      this.launcherOpen = false;
      this.runningOpen = true;
      this.runningMinimized = false;
      await this.refreshLiveRun();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      arenaError("store: startRun failed", e);
      this.launcherOpen = true;
      if (!msg.includes("exceeds limit")) {
        app.showToast("error", msg);
      }
      throw e;
    } finally {
      this.loading = false;
    }
  }

  async acceptFindings(runId: string, findingIds?: string[]) {
    try {
      const n = await invoke<number>("arena_accept_findings", {
        req: { run_id: runId, finding_ids: findingIds },
      });
      app.showToast("success", `Accepted ${n} finding(s) into Review`);
      if (this.overlayRunId === runId) {
        await this.refreshOverlayRun();
      }
      return n;
    } catch (e) {
      app.showToast("error", e instanceof Error ? e.message : String(e));
      throw e;
    }
  }

  /** Import a completed single review into `.er/review.json` and focus the Review tab filter. */
  private async importSingleReviewToTab(runId: string, snap: ArenaRunSnapshot) {
    const label = agentLabelFromSnapshot(snap);
    try {
      const n = await invoke<number>("arena_accept_findings", {
        req: { run_id: runId, finding_ids: undefined },
      });
      aiReviewFilter.filter = label;
      if (n > 0) {
        app.showToast(
          "success",
          `${label} review complete — ${n} finding${n === 1 ? "" : "s"} in Review`,
        );
      } else {
        app.showToast("info", `${label} review complete — no new findings to import`);
      }
      arenaLog("store: single review imported to Review tab", { runId, label, n });
    } catch (e) {
      arenaError("store: importSingleReviewToTab failed", e);
      app.showToast("error", e instanceof Error ? e.message : String(e));
    }
  }

  private openArenaOverlayForRun(runId: string, snap: ArenaRunSnapshot) {
    this.overlayRunId = runId;
    this.overlaySnapshot = snap;
    this.overlayOpen = true;
    arenaLog("store: arena complete — opening overlay", { runId });
  }

  async refreshLiveRun() {
    if (this.liveRuns.length > 1) {
      await this.refreshAllLiveRuns();
      return;
    }
    const id = this.liveRunId;
    if (!id) {
      arenaWarn("store: refreshLiveRun skipped — no liveRunId");
      return;
    }
    arenaLog("store: refreshLiveRun", { runId: id });
    try {
      const [snap, prog] = await Promise.all([
        invoke<ArenaRunSnapshot>("arena_get", { runId: id }),
        invoke<ArenaProgressState>("arena_progress", { runId: id }),
      ]);
      this.liveSnapshot = normalizeArenaSnapshot(snap);
      this.progress = prog;
      this.liveRunStates = { [id]: { snapshot: this.liveSnapshot, progress: prog } };
      this.finishLiveRunIfTerminal(id, this.liveSnapshot.run.status);
    } catch (e) {
      arenaError("store: refreshLiveRun failed", e);
      app.showToast("error", e instanceof Error ? e.message : String(e));
    }
  }

  private finishLiveRunIfTerminal(
    runId: string,
    status: ArenaRunSnapshot["run"]["status"],
  ) {
    arenaLog("store: refreshLiveRun status", { runId, status });
    if (status !== "complete" && status !== "failed" && status !== "cancelled") {
      return;
    }
    if (this.liveRuns.length > 1) {
      return;
    }
    this.runningOpen = false;
    this.runningMinimized = false;
    this.runStartedAt = null;
    const snap = this.liveSnapshot;
    if (status === "complete" && snap) {
      if (isSingleReviewRun(this.lastConfig, snap)) {
        void this.importSingleReviewToTab(runId, snap);
      } else if (isArenaRunFromSnapshot(snap)) {
        this.openArenaOverlayForRun(runId, snap);
      } else {
        void this.importSingleReviewToTab(runId, snap);
      }
    } else if (status === "failed") {
      arenaWarn("store: run ended", { status });
      const label = snap ? agentLabelFromSnapshot(snap) : "Review";
      app.showToast(
        "error",
        isSingleReviewRun(this.lastConfig, snap)
          ? `${label} review failed — check provider logs`
          : "Arena run failed — check provider logs",
      );
    }
  }

  private finishBatchIfAllTerminal() {
    const states = this.liveRuns
      .map((e) => this.liveRunStates[e.runId]?.snapshot?.run.status)
      .filter(Boolean);
    if (states.length !== this.liveRuns.length) return;
    if (!states.every((s) => s === "complete" || s === "failed" || s === "cancelled")) {
      return;
    }
    this.runningOpen = false;
    this.runningMinimized = false;
    this.runStartedAt = null;
    const completed = this.liveRuns.filter(
      (e) => this.liveRunStates[e.runId]?.snapshot?.run.status === "complete",
    );
    const arenaComplete = completed.filter((e) => {
      const snap = this.liveRunStates[e.runId]?.snapshot;
      return snap && isArenaRunFromSnapshot(snap);
    });
    const singleComplete = completed.filter((e) => {
      const snap = this.liveRunStates[e.runId]?.snapshot;
      return snap && !isArenaRunFromSnapshot(snap);
    });
    if (arenaComplete.length > 0) {
      const first = arenaComplete[0]!;
      const snap = this.liveRunStates[first.runId]?.snapshot;
      if (snap) {
        this.openArenaOverlayForRun(first.runId, snap);
      }
    }
    for (const entry of singleComplete) {
      const snap = this.liveRunStates[entry.runId]?.snapshot;
      if (snap) {
        void this.importSingleReviewToTab(entry.runId, snap);
      }
    }
    if (states.some((s) => s === "failed")) {
      app.showToast("error", "One or more arena runs failed — check provider logs");
    }
  }

  async refreshAllLiveRuns() {
    if (this.liveRuns.length === 0) return;
    arenaLog("store: refreshAllLiveRuns", { count: this.liveRuns.length });
    const next: Record<string, LiveRunState> = { ...this.liveRunStates };
    await Promise.all(
      this.liveRuns.map(async (entry) => {
        try {
          const [snap, prog] = await Promise.all([
            invoke<ArenaRunSnapshot>("arena_get", { runId: entry.runId }),
            invoke<ArenaProgressState>("arena_progress", { runId: entry.runId }),
          ]);
          next[entry.runId] = {
            snapshot: normalizeArenaSnapshot(snap),
            progress: prog,
          };
        } catch (e) {
          arenaError("store: refreshAllLiveRuns failed", { runId: entry.runId, e });
        }
      }),
    );
    this.liveRunStates = next;
    const active = this.liveRuns.find((e) => {
      const st = next[e.runId]?.snapshot?.run.status;
      return st != null && isArenaRunActive(st);
    });
    const focus = active ?? this.liveRuns[0];
    if (focus) {
      const st = next[focus.runId];
      if (st?.snapshot) {
        this.liveRunId = focus.runId;
        this.liveSnapshot = st.snapshot;
        this.progress = st.progress;
      }
    }
    this.finishBatchIfAllTerminal();
  }

  minimizeRunning() {
    if (this.runningOpen) this.runningMinimized = true;
  }

  restoreRunning() {
    if (this.runningOpen) this.runningMinimized = false;
  }

  showRunningProgress() {
    const active = app.snapshot?.active_arena_run ?? this.liveRunId;
    if (!active) return;
    this.liveRunId = active;
    this.runningOpen = true;
    this.runningMinimized = false;
    this.overlayOpen = false;
    void this.refreshLiveRun();
  }

  async skipToResults() {
    if (this.liveRunId) await this.refreshLiveRun();
    const status = this.liveSnapshot?.run.status;
    if (status !== "complete") {
      app.showToast(
        "info",
        "Run still in progress — wait for completion or cancel",
      );
      return;
    }
    this.runningOpen = false;
    this.runningMinimized = false;
    const snap = this.liveSnapshot;
    const runId = this.liveRunId;
    if (!snap || !runId) return;
    if (isSingleReviewRun(this.lastConfig, snap)) {
      void this.importSingleReviewToTab(runId, snap);
    } else {
      this.openArenaOverlayForRun(runId, snap);
    }
  }

  async cancelRun() {
    const ids =
      this.liveRuns.length > 0
        ? this.liveRuns.map((e) => e.runId)
        : this.liveRunId
          ? [this.liveRunId]
          : [];
    if (ids.length === 0) return;
    arenaLog("store: cancelRun", { runIds: ids });
    try {
      await Promise.all(ids.map((runId) => invoke("arena_cancel", { runId })));
      this.runningOpen = false;
      this.runningMinimized = false;
      this.runStartedAt = null;
      if (this.liveRuns.length > 1) {
        await this.refreshAllLiveRuns();
      } else {
        await this.refreshLiveRun();
      }
    } catch (e) {
      app.showToast("error", String(e));
    }
  }

  async deleteRun(runId: string) {
    const sum = this.summaries.find((s) => s.id === runId);
    if (sum && isArenaRunActive(sum.status)) {
      app.showToast("error", "Cancel the run before deleting it");
      return;
    }
    if (!confirm("Delete this arena run permanently? This cannot be undone.")) {
      return;
    }
    try {
      await invoke("arena_delete", { runId });
      if (this.overlayRunId === runId) {
        this.overlayOpen = false;
        this.overlayRunId = null;
        this.overlaySnapshot = null;
      }
      if (this.liveRunId === runId) {
        this.liveRunId = null;
        this.runningOpen = false;
        this.runningMinimized = false;
      }
      app.showToast("success", "Arena run deleted");
    } catch (e) {
      app.showToast("error", e instanceof Error ? e.message : String(e));
    }
  }

  async openOverlay(runId?: string) {
    const id = runId ?? this.branchSummaries[0]?.id;
    if (!id) return;
    try {
      const snap = normalizeArenaSnapshot(
        await invoke<ArenaRunSnapshot>("arena_get", { runId: id }),
      );
      if (isArenaRunFromSnapshot(snap)) {
        this.openArenaOverlayForRun(id, snap);
      } else {
        await this.importSingleReviewToTab(id, snap);
      }
    } catch (e) {
      arenaError("store: openOverlay failed", e);
      app.showToast("error", String(e));
    }
  }

  closeOverlay() {
    this.overlayOpen = false;
    const active = app.snapshot?.active_arena_run;
    if (active && this.hasLiveRun) {
      this.liveRunId = active;
      this.runningOpen = true;
      void this.refreshLiveRun();
    }
  }

  onRevision() {
    if (this.liveRunId && (this.runningOpen || this.runningMinimized)) {
      arenaLog("store: onRevision → refreshLiveRun");
      void this.refreshLiveRun();
    } else if (this.overlayOpen && this.overlayRunId) {
      void this.refreshOverlayRun();
    }
  }

  async refreshOverlayRun() {
    const id = this.overlayRunId;
    if (!id) return;
    try {
      const snap = await invoke<ArenaRunSnapshot>("arena_get", { runId: id });
      this.overlaySnapshot = normalizeArenaSnapshot(snap);
    } catch (e) {
      arenaError("store: refreshOverlayRun failed", e);
    }
  }

  syncFromSnapshot() {
    const active = app.snapshot?.active_arena_run ?? null;
    if (active === this.liveRunId) return;

    if (!active && this.liveRunId && (this.runningOpen || this.runningMinimized)) {
      return;
    }

    arenaLog("store: syncFromSnapshot", { active, previous: this.liveRunId });
    if (active) {
      this.liveRunId = active;
      if (!this.runningOpen && !this.overlayOpen) {
        this.runningOpen = true;
        this.runStartedAt ??= Date.now();
      }
      if (this.runningOpen || this.runningMinimized) {
        void this.refreshLiveRun();
      }
    } else if (!this.runningOpen && !this.runningMinimized) {
      this.liveRunId = null;
      this.runStartedAt = null;
    }
  }
}

function scopeFromMode(mode: string | undefined): ArenaScope {
  if (mode === "unstaged") return "unstaged";
  if (mode === "staged") return "staged";
  return "branch";
}

export const arena = new ArenaStore();
