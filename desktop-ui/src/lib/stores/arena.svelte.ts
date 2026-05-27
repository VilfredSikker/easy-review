import { invoke } from "@tauri-apps/api/core";
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
    return app.snapshot?.arena_enabled ?? false;
  }

  get summaries(): ArenaRunSummary[] {
    return app.snapshot?.arena_runs ?? [];
  }

  openLauncher(preset?: ReviewerRef[]) {
    if (!this.enabled) {
      app.showToast("info", "Enable features.arena in .er-config.toml to use AI Review Arena");
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
    if (!this.enabled) return;
    this.loading = true;
    this.lastConfig = config;
    try {
      const runId = await invoke<string>("arena_start", {
        req: {
          title: config.title,
          reviewers: config.reviewers,
          scope: config.scope,
          files: config.files,
          confirm: config.confirm ?? false,
        },
      });
      this.activeRunId = runId;
      this.launcherOpen = false;
      this.runningOpen = true;
      this.runningMinimized = false;
      await this.refreshRun();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      app.showToast("error", msg);
    } finally {
      this.loading = false;
    }
  }

  async refreshRun() {
    const id = this.activeRunId;
    if (!id) return;
    try {
      const snap = await invoke<ArenaRunSnapshot>("arena_get", { runId: id });
      this.snapshot = normalizeArenaSnapshot(snap);
      const status = this.snapshot.run.status;
      if (status === "complete" || status === "failed" || status === "cancelled") {
        this.runningOpen = false;
        this.runningMinimized = false;
        if (status === "complete") {
          this.overlayOpen = true;
        }
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      app.showToast("error", msg);
    }
  }

  async cancelRun() {
    const id = this.activeRunId;
    if (!id) return;
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
      void this.refreshRun();
    }
  }

  syncFromSnapshot() {
    const active = app.snapshot?.active_arena_run;
    if (active && !this.activeRunId) {
      this.activeRunId = active;
    }
  }
}

function scopeFromMode(mode: string | undefined): "branch" | "unstaged" | "staged" {
  if (mode === "unstaged") return "unstaged";
  if (mode === "staged") return "staged";
  return "branch";
}

export const arena = new ArenaStore();
