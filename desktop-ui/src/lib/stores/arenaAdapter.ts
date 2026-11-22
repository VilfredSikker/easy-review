import type { ArenaPlan } from "$lib/protocol/generated/ArenaPlan";
import type { ArenaReviewer } from "$lib/protocol/generated/ArenaReviewer";
import type { ArenaRunJob } from "$lib/protocol/generated/ArenaRunJob";
import type { ArenaRunRow } from "$lib/protocol/generated/ArenaRunRow";
import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { Invoke, ResourceClient } from "$lib/protocol/client";
import type { AppSnapshot } from "$lib/types";
import type {
  ArenaEstimate,
  ArenaProgressState,
  ArenaRunSnapshot,
  ArenaRunSummary,
  ReviewerRef,
} from "$lib/types/arena";
import { JOBS } from "./annotationsAdapter";
import type { ResourceStore } from "./resourceStore";

/** A run to price or start, as the launcher sends it. */
export interface ArenaRequest {
  scope: string;
  files?: string[];
  rounds?: number;
  arbiter?: ReviewerRef;
  reviewers?: ReviewerRef[];
  groups?: { agent_kind: string; models: ReviewerRef[]; title?: string }[];
}

export interface ArenaStartRequest extends ArenaRequest {
  title?: string;
  confirm?: boolean;
  agent_kind?: string;
  effort?: string;
}

interface ArenaAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "send">;
  client: Pick<ResourceClient, "fetch">;
  /** The registry's active tab, the one the page paints. */
  activeKey: () => TabKey | null;
  invoke: Invoke;
  /** Called after a write that changes the active tab's run list, which nothing pushes. */
  runsMoved?: () => void;
}

function reviewer(ref: ReviewerRef): ArenaReviewer {
  return { provider_id: ref.provider_id, model_id: ref.model_id, agent_kind: ref.agent_kind ?? null };
}

function plan(req: ArenaRequest): ArenaPlan {
  return {
    scope: req.scope,
    files: req.files ?? null,
    rounds: req.rounds ?? null,
    arbiter: req.arbiter ? reviewer(req.arbiter) : null,
    reviewers: (req.reviewers ?? []).map(reviewer),
    groups: (req.groups ?? []).map((group) => ({
      agent_kind: group.agent_kind,
      models: group.models.map(reviewer),
      title: group.title ?? null,
    })),
  };
}

/** A run's line as the arena types describe it. */
export function summaryOf(row: ArenaRunRow): ArenaRunSummary {
  return {
    id: row.id,
    title: row.title ?? undefined,
    branch_ref: row.branch_ref,
    status: row.status,
    created_at: row.created_at,
    completed_at: row.completed_at ?? undefined,
    reviewer_count: row.reviewer_count,
    finding_count: row.finding_count,
  };
}

/** A live run as the running pill lists it; the record's title and counts are not part of `jobs`. */
function liveSummary(job: ArenaRunJob): ArenaRunSummary {
  return {
    id: job.run_id,
    branch_ref: "",
    status: job.status,
    created_at: "",
    reviewer_count: 0,
    finding_count: 0,
  };
}

function live(job: ArenaRunJob): boolean {
  return job.status === "queued" || (typeof job.status === "object" && "running" in job.status);
}

/**
 * The arena's commands and reads: the Tauri commands with the flag off, and
 * with it on `er_send` and `er_fetch` keyed to the active tab, with progress
 * and the live runs read from `jobs`.
 */
export class ArenaAdapter {
  constructor(private readonly deps: ArenaAdapterDeps) {}

  private tab(): TabKey {
    const tab = this.deps.activeKey();
    if (tab === null) throw new Error("No tab is open");
    return tab;
  }

  private async fetch(key: ResourceKey): Promise<Resource | undefined> {
    const [entry] = await this.deps.client.fetch([key]);
    return entry?.value;
  }

  private async send(command: Command): Promise<string | null> {
    return (await this.deps.store.send(command)).created;
  }

  /** Sends a write that adds or removes a run, then has the run list read again. */
  private async sendMoving(command: Command): Promise<string | null> {
    const created = await this.send(command);
    this.deps.runsMoved?.();
    return created;
  }

  /** The live runs `jobs` holds; empty with the flag off. */
  liveRuns(): ArenaRunJob[] {
    const held = this.deps.flagOn ? this.deps.store.get(JOBS)?.value : undefined;
    return held?.kind === "jobs" ? held.arena_runs : [];
  }

  /** `jobs`' version, which moves with every live run's progress; null with the flag off. */
  jobsVersion(): number | null {
    return this.deps.flagOn ? (this.deps.store.get(JOBS)?.version ?? null) : null;
  }

  /** `raw` with the run lists from the resources, which the App's snapshot no longer feeds with the flag on. */
  overlay(raw: AppSnapshot, runs: ArenaRunSummary[]): AppSnapshot {
    return {
      ...raw,
      active_arena_run: null,
      arena_runs: runs,
      background_arena_runs: this.liveRuns().filter(live).map(liveSummary).reverse(),
    };
  }

  async estimate(req: ArenaRequest): Promise<ArenaEstimate> {
    if (!this.deps.flagOn) {
      const command = req.groups?.length ? "arena_estimate_batch" : "arena_estimate";
      return this.deps.invoke<ArenaEstimate>(command, { req });
    }
    const value = await this.fetch({ kind: "arena_estimate", tab: this.tab(), plan: plan(req) });
    if (value?.kind !== "arena_estimate") throw new Error("The estimate did not answer");
    return value;
  }

  /** Starts one run, or one per agent group; answers the run ids. */
  async start(req: ArenaStartRequest): Promise<string[]> {
    if (!this.deps.flagOn) {
      if (req.groups?.length) return this.deps.invoke<string[]>("arena_start_batch", { req });
      return [await this.deps.invoke<string>("arena_start", { req })];
    }
    const created = await this.sendMoving({
      kind: "arena_start",
      tab: this.tab(),
      plan: plan(req),
      title: req.title ?? null,
      confirm: req.confirm ?? false,
      agentKind: req.agent_kind ?? null,
      effort: req.effort ?? null,
    });
    return created ? created.split("\n") : [];
  }

  async validateFindings(): Promise<string | null> {
    if (!this.deps.flagOn) return this.deps.invoke<string | null>("arena_validate_findings", {});
    return this.send({ kind: "arena_validate_findings", tab: this.tab() });
  }

  /** Imports a run's findings; answers how many it took. */
  async acceptFindings(runId: string, findingIds?: string[]): Promise<number> {
    if (!this.deps.flagOn) {
      return this.deps.invoke<number>("arena_accept_findings", {
        req: { run_id: runId, finding_ids: findingIds },
      });
    }
    const created = await this.sendMoving({
      kind: "arena_accept_findings",
      tab: this.tab(),
      runId,
      findingIds: findingIds ?? null,
    });
    return Number(created ?? 0);
  }

  async get(runId: string): Promise<ArenaRunSnapshot> {
    if (!this.deps.flagOn) return this.deps.invoke<ArenaRunSnapshot>("arena_get", { runId });
    const value = await this.fetch({ kind: "arena_run", tab: this.tab(), run_id: runId });
    if (value?.kind !== "arena_run") throw new Error("The run did not answer");
    return JSON.parse(value.json) as ArenaRunSnapshot;
  }

  /** A live run's progress: from `jobs` with the flag on, empty for a run nothing runs. */
  async progress(runId: string): Promise<ArenaProgressState> {
    if (!this.deps.flagOn) return this.deps.invoke<ArenaProgressState>("arena_progress", { runId });
    const job = this.liveRuns().find((run) => run.run_id === runId);
    return job ? job.progress : { round: 0, total_rounds: 0, phase: "", thinking: [], done: [] };
  }

  /** The active tab's runs on its branch, newest first; with the flag off the snapshot carries them. */
  async runs(): Promise<ArenaRunSummary[]> {
    const value = await this.fetch({ kind: "arena_runs", tab: this.tab() });
    return value?.kind === "arena_runs" ? value.runs.map(summaryOf) : [];
  }

  async cancel(runId: string): Promise<void> {
    if (!this.deps.flagOn) {
      await this.deps.invoke("arena_cancel", { runId });
      return;
    }
    await this.send({ kind: "arena_cancel", tab: this.tab(), runId });
  }

  async delete(runId: string): Promise<void> {
    if (!this.deps.flagOn) {
      await this.deps.invoke("arena_delete", { runId });
      return;
    }
    await this.sendMoving({ kind: "arena_delete", tab: this.tab(), runId });
  }

  /** Records the reviewer's verdict, then reads the run back. */
  async override(runId: string, findingId: string, verdict: string, note: string): Promise<ArenaRunSnapshot> {
    if (!this.deps.flagOn) {
      return this.deps.invoke<ArenaRunSnapshot>("arena_override", {
        req: { run_id: runId, finding_id: findingId, verdict, note },
      });
    }
    await this.send({ kind: "arena_override", tab: this.tab(), runId, findingId, verdict, note });
    return this.get(runId);
  }
}
