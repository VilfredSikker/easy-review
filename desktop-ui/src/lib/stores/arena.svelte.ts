import { invoke } from "@tauri-apps/api/core";
import { arenaError, arenaLog, arenaWarn } from "$lib/arena/log";
import { normalizeArenaSnapshot } from "$lib/arena/normalize";
import { app } from "$lib/stores/app.svelte";
import type { ArenaRunSnapshot, ArenaRunSummary, ReviewerRef } from "$lib/types/arena";

export interface ArenaStartConfig {
  title?: string;
  reviewers: ReviewerRef[];
  scope: "branch" | "unstaged" | "staged";
  rounds?: number;
  files?: string[];
  confirm?: boolean;
}

class ArenaStore {
  launcherOpen = $state(false);
  runningOpen = $state(false);
  runningMinimized = $state(false);
  overlayOpen = $state(false);
  activeRunId = $state<string | null>(null);
  lastConfig = $state<ArenaStartConfig | null>(null);
  snapshot = $state<ArenaRunSnapshot | null>(null);
  layoutMode = $state<"bracket" | "matrix" | "funnel">("bracket");
  loading = $state(false);

  get enabled(): boolean {
    return app.snapshot?.arena_enabled ?? true;
  }

  get summaries(): ArenaRunSummary[] {
    return app.snapshot?.arena_runs ?? [];
  }

  openLauncher(preset?: ReviewerRef[]) {
    arenaLog("store: openLauncher", { enabled: this.enabled, preset: preset?.length ?? 0 });
    if (!this.enabled) {
      arenaWarn("store: openLauncher blocked — arena disabled");
      return;
    }
    if (preset?.length) {
      this.lastConfig = {
        reviewers: preset,
        scope: scopeFromMode(app.snapshot?.mode),
        rounds: preset.length >= 2 ? 3 : 1,
      };
    }
    this.launcherOpen = true;
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
    const payload = {
      req: {
        title: config.title,
        reviewers: config.reviewers,
        scope: config.scope,
        files: config.files,
        rounds: config.rounds != null ? Number(config.rounds) : undefined,
        confirm: config.confirm ?? false,
      },
    };
    arenaLog("store: invoking arena_start", payload);
    try {
      const runId = await invoke<string>("arena_start", payload);
      arenaLog("store: arena_start ok", { runId });
      this.activeRunId = runId;
      this.launcherOpen = false;
      this.runningOpen = true;
      this.runningMinimized = false;
      await this.refreshRun();
      arenaLog("store: run started, running panel open", {
        runningOpen: this.runningOpen,
        status: this.snapshot?.run.status,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      arenaError("store: arena_start failed", e);
      app.showToast("error", msg);
    } finally {
      this.loading = false;
    }
  }

  async refreshRun() {
    const id = this.activeRunId;
    if (!id) {
      arenaWarn("store: refreshRun skipped — no activeRunId");
      return;
    }
    arenaLog("store: refreshRun", { runId: id });
    try {
      const snap = await invoke<ArenaRunSnapshot>("arena_get", { runId: id });
      this.snapshot = normalizeArenaSnapshot(snap);
      const status = this.snapshot.run.status;
      arenaLog("store: refreshRun status", { runId: id, status });
      if (status === "complete" || status === "failed" || status === "cancelled") {
        this.runningOpen = false;
        this.runningMinimized = false;
        if (status === "complete") {
          this.overlayOpen = true;
          arenaLog("store: run complete — opening overlay");
        } else {
          arenaWarn("store: run ended", { status });
        }
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      arenaError("store: arena_get failed", e);
      app.showToast("error", msg);
    }
  }

  async cancelRun() {
    const id = this.activeRunId;
    if (!id) return;
    arenaLog("store: cancelRun", { runId: id });
    try {
      await invoke("arena_cancel", { runId: id });
      this.runningOpen = false;
      this.runningMinimized = false;
      await this.refreshRun();
    } catch (e) {
      app.showToast("error", String(e));
    }
  }

  openOverlay(runId?: string) {
    if (runId) this.activeRunId = runId;
    if (!this.activeRunId) {
      const latest = this.summaries[0];
      if (latest) this.activeRunId = latest.id;
    }
    if (!this.activeRunId) return;
    void this.refreshRun().then(() => {
      this.overlayOpen = true;
    });
  }

  closeOverlay() {
    this.overlayOpen = false;
  }

  onRevision() {
    if (this.activeRunId && (this.runningOpen || this.overlayOpen)) {
      arenaLog("store: onRevision → refreshRun");
      void this.refreshRun();
    }
  }

  syncFromSnapshot() {
    const active = app.snapshot?.active_arena_run ?? null;
    if (active === this.activeRunId) return;
    arenaLog("store: syncFromSnapshot", { active, previous: this.activeRunId });
    this.activeRunId = active;
    if (active && (this.runningOpen || this.overlayOpen)) {
      void this.refreshRun();
    } else if (!active) {
      this.runningOpen = false;
      this.runningMinimized = false;
    }
  }
}

function scopeFromMode(mode: string | undefined): "branch" | "unstaged" | "staged" {
  if (mode === "unstaged") return "unstaged";
  if (mode === "staged") return "staged";
  return "branch";
}

export const arena = new ArenaStore();
