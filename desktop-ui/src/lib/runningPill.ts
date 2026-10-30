import { reviewRunVisible } from "./reviewRunCard";
import { activeJobCount, type RunningJob } from "./runningJobs";
import type { AgentCommandStatus, BackgroundTaskSnapshot, ReviewFlowSnapshot } from "./types";

export type PillTone = "running" | "need" | "done" | "failed";

export interface RunningPillView {
  tone: PillTone;
  text: string;
  /** Other running jobs beside the review, shown as "+N". */
  extra: number;
}

const FLOW_STATE_LABEL: Partial<Record<ReviewFlowSnapshot["state"], string>> = {
  triaging: "triaging",
  finding: "finding",
  verifying: "verifying",
};

/** Agent commands minus the copies of background tasks. The snapshot mirrors
 *  each background task on the active tab into `agent_commands` under its
 *  kind, for the per-tab badges; counting both would show one job as two. A
 *  copy carries the task's kind and status verbatim, so match on both. */
export function ownAgentCommands(
  commands: readonly AgentCommandStatus[] | null | undefined,
  tasks: readonly BackgroundTaskSnapshot[] | null | undefined,
): AgentCommandStatus[] {
  const mirrored = new Set((tasks ?? []).map((t) => `${t.kind}\u0000${t.status}`));
  return (commands ?? []).filter((c) => !mirrored.has(`${c.name}\u0000${c.status}`));
}

/** Jobs that are not part of the review flow. Its finders run as background
 *  tasks and a Deep review runs as an arena run; both are already drawn on the
 *  review card, so listing them again would count the same work twice. */
export function otherJobs(
  jobs: readonly RunningJob[],
  flow: ReviewFlowSnapshot | null | undefined,
): RunningJob[] {
  if (!flow) return [...jobs];
  const owned = new Set<string>();
  for (const t of flow.tasks ?? []) {
    if (t.background_task_id) owned.add(`task:${t.background_task_id}`);
  }
  if (flow.arena_run_id) owned.add(`arena:${flow.arena_run_id}`);
  return jobs.filter((j) => !owned.has(j.id));
}

/** What the top bar's AI pill says, or null when it should be hidden. A
 *  review flow wins because it is the one job that can wait on the user. */
export function runningPillView(
  flow: ReviewFlowSnapshot | null | undefined,
  jobs: readonly RunningJob[],
): RunningPillView | null {
  const others = otherJobs(jobs, flow);
  const active = activeJobCount(others);
  if (reviewRunVisible(flow)) {
    if (flow.state === "awaiting_confirmation") {
      return { tone: "need", text: "Review needs confirmation", extra: active };
    }
    if (flow.state === "done") {
      return { tone: "need", text: `Review · ${flow.awaiting_accept} to accept`, extra: active };
    }
    // Busy outside the three working states means the flow is draining.
    return { tone: "running", text: `Review · ${FLOW_STATE_LABEL[flow.state] ?? "stopping"}`, extra: active };
  }
  if (active > 0) return { tone: "running", text: `${active} running`, extra: 0 };
  if (others.length === 0) return null;
  if (others.some((j) => j.status === "failed")) return { tone: "failed", text: "AI failed", extra: 0 };
  return { tone: "done", text: "AI done", extra: 0 };
}

export interface ReviewRing {
  /** Stroke length of the progress arc on a circle of `circumference`. */
  arc: number;
  circumference: number;
  label: string;
  complete: boolean;
}

/** Reviewed-file ring. Real counts from the snapshot, so a determinate arc. */
export function reviewRing(reviewed: number, total: number, radius = 6): ReviewRing {
  const circumference = 2 * Math.PI * radius;
  const done = Math.max(0, Math.min(reviewed, total));
  return {
    arc: total > 0 ? (circumference * done) / total : 0,
    circumference,
    label: `${done}/${total} reviewed`,
    complete: total > 0 && done === total,
  };
}
