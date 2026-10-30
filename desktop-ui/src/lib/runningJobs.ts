import type { ArenaRunSummary } from "./types/arena";
import type {
  AgentCommandStatus,
  BackgroundTaskSnapshot,
  TabSummary,
} from "./types";

/** How long a finished job stays listed so the pill can show "done" before it
 *  disappears; matches the prototype. */
export const DONE_WINDOW_MS = 8000;

export type JobSource = "background_task" | "agent_command" | "arena";
export type JobStatus = "queued" | "running" | "done" | "failed";

export interface RunningJob {
  id: string;
  source: JobSource;
  label: string;
  status: JobStatus;
  /** Tab the job belongs to; null when it cannot be attributed to one. */
  tabIdx: number | null;
  startedAtMs: number | null;
  finishedAtMs: number | null;
  error: string | null;
}

export interface JobsInput {
  backgroundTasks?: readonly BackgroundTaskSnapshot[];
  /** Commands of the active tab only; the snapshot carries no tab for them. */
  agentCommands?: readonly AgentCommandStatus[];
  arenaRuns?: readonly ArenaRunSummary[];
  tabs: readonly TabSummary[];
  activeTabIdx: number | null;
  nowMs: number;
  /** Agent commands carry no timestamps, so the caller records when it first
   *  saw each one finish. Without it a finished command is dropped at once. */
  commandDoneAtMs?: ReadonlyMap<string, number>;
}

function taskStatus(s: string): JobStatus {
  return s === "queued" || s === "done" || s === "failed" ? s : "running";
}

function matchTab(
  tabs: readonly TabSummary[],
  t: BackgroundTaskSnapshot,
): number | null {
  if (t.pr_number != null) {
    const hit = tabs.find(
      (x) =>
        x.pr_number === t.pr_number &&
        (!t.remote_repo ||
          x.remote?.toLowerCase() === t.remote_repo.toLowerCase()),
    );
    if (hit) return hit.idx;
  }
  const byBranch = tabs.find(
    (x) =>
      x.repo_root === t.repo_root &&
      (x.branch === t.branch_label || x.label === t.branch_label),
  );
  if (byBranch) return byBranch.idx;
  return tabs.find((x) => x.repo_root === t.repo_root)?.idx ?? null;
}

function arenaStatus(s: ArenaRunSummary["status"]): JobStatus {
  if (s === "queued") return "queued";
  if (s === "complete") return "done";
  if (s === "failed" || s === "cancelled") return "failed";
  return "running";
}

function isActive(s: JobStatus): boolean {
  return s === "queued" || s === "running";
}

/** One list over the three places the backend reports AI work. Active jobs are
 *  always kept; finished ones only inside the done window, so the run pill
 *  settles instead of lingering. Newest first. */
export function aggregateJobs(input: JobsInput): RunningJob[] {
  const { nowMs } = input;
  const jobs: RunningJob[] = [];

  for (const t of input.backgroundTasks ?? []) {
    jobs.push({
      id: `task:${t.id}`,
      source: "background_task",
      label: t.label,
      status: taskStatus(t.status),
      tabIdx: matchTab(input.tabs, t),
      startedAtMs: t.started_at_ms,
      finishedAtMs: t.finished_at_ms ?? null,
      error: t.error ?? null,
    });
  }

  for (const c of input.agentCommands ?? []) {
    const status = taskStatus(c.status);
    jobs.push({
      id: `cmd:${c.name}`,
      source: "agent_command",
      label: c.name,
      status,
      tabIdx: input.activeTabIdx,
      startedAtMs: null,
      finishedAtMs: isActive(status)
        ? null
        : (input.commandDoneAtMs?.get(c.name) ?? null),
      error: c.error ?? null,
    });
  }

  for (const r of input.arenaRuns ?? []) {
    const status = arenaStatus(r.status);
    const started = Date.parse(r.created_at);
    const finished = r.completed_at ? Date.parse(r.completed_at) : NaN;
    jobs.push({
      id: `arena:${r.id}`,
      source: "arena",
      label: r.title ?? "Arena run",
      status,
      tabIdx: input.tabs.find((t) => t.branch === r.branch_ref)?.idx ?? null,
      startedAtMs: Number.isNaN(started) ? null : started,
      finishedAtMs: Number.isNaN(finished) ? null : finished,
      error: null,
    });
  }

  return jobs
    .filter(
      (j) =>
        isActive(j.status) ||
        (j.finishedAtMs != null && nowMs - j.finishedAtMs < DONE_WINDOW_MS),
    )
    .sort((a, b) => (b.startedAtMs ?? 0) - (a.startedAtMs ?? 0));
}

export function activeJobCount(jobs: readonly RunningJob[]): number {
  return jobs.filter((j) => isActive(j.status)).length;
}

export function jobsForTab(
  jobs: readonly RunningJob[],
  tabIdx: number,
): RunningJob[] {
  return jobs.filter((j) => j.tabIdx === tabIdx);
}
