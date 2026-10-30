import { commitColorVar } from "./commitColors";
import { reviewFlowControls, reviewFlowEstimate, reviewFlowProgressView } from "./reviewFlow";
import type { ReviewFlowSnapshot, ReviewFlowTaskProgress } from "./types";

export type RunStage = ReviewFlowTaskProgress["stage"];
export type StepState = "waiting" | "active" | "done" | "failed" | "skipped";

export interface RunStep {
  stage: RunStage;
  label: string;
  state: StepState;
  /** Share of the stage's task rows that finished, 0..1. Counted, never
   *  estimated: the backend reports no per-task percentage. */
  fill: number;
}

export interface RunTaskRow {
  task: ReviewFlowTaskProgress;
  title: string;
  /** "round 2/3" when the task runs several rounds. */
  round: string | null;
  statusLabel: string;
  /** Provider and model, e.g. "OpenAI · GPT-5". */
  model: string;
  dotColor: string;
}

export interface RunTaskGroup {
  stage: RunStage;
  label: string;
  /** "3 lenses · deep" beside the Findings heading. */
  note: string | null;
  rows: RunTaskRow[];
}

export interface ReviewRunCard {
  depth: ReviewFlowSnapshot["depth"];
  head: string;
  sub: string;
  steps: RunStep[];
  confirm: { lenses: { id: string; label: string; color: string }[]; estimate: string | null } | null;
  done: { kept: number; dropped: number; awaitingAccept: number; finders: number } | null;
  groups: RunTaskGroup[];
  /** Run id whose results wait in the verification overlay. */
  openRunId: string | null;
  canCancel: boolean;
  canConfirm: boolean;
  /** A terminal flow can be started again, which is what Retry means here:
   *  the backend has no per-finder retry. */
  canRestart: boolean;
}

const STAGES: { stage: RunStage; label: string }[] = [
  { stage: "triage", label: "Triage" },
  { stage: "finding", label: "Findings" },
  { stage: "verifying", label: "Verification" },
];

const STATUS_LABEL: Record<ReviewFlowTaskProgress["status"], string> = {
  waiting: "Waiting",
  queued: "Queued",
  running: "Running",
  done: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
  skipped: "Skipped",
};

const FINISHED = new Set<ReviewFlowTaskProgress["status"]>(["done", "failed", "cancelled", "skipped"]);

/** Lens ids are slugs ("security", "data-model"); show them as words. */
export function lensLabel(lens: string): string {
  const words = lens.replace(/[-_]+/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : lens;
}

/** One colour per lens, stable for the run: its position in `flow.lenses`.
 *  Triage and the arbiter have no lens and stay neutral. */
export function lensColor(flow: ReviewFlowSnapshot, lens: string | null): string {
  if (!lens) return "var(--color-fg-3)";
  const i = flow.lenses.indexOf(lens);
  return i < 0 ? "var(--color-fg-3)" : commitColorVar(i);
}

/** Stage order of the flow state; `awaiting_confirmation` sits between triage
 *  and findings, so triage reads as done while findings wait. */
function stateStep(state: ReviewFlowSnapshot["state"]): number {
  switch (state) {
    case "triaging":
      return 0;
    case "awaiting_confirmation":
      return 0.5;
    case "finding":
      return 1;
    case "verifying":
      return 2;
    case "done":
      return 3;
    default:
      return -1;
  }
}

function steps(flow: ReviewFlowSnapshot): RunStep[] {
  const tasks = flow.tasks ?? [];
  const rail = reviewFlowProgressView(flow)?.rail;
  const at = stateStep(flow.state);
  return STAGES.map(({ stage, label }, i) => {
    const own = tasks.filter((t) => t.stage === stage);
    const finished = own.filter((t) => FINISHED.has(t.status)).length;
    let state: StepState = rail?.find((r) => r.stage === stage)?.state ?? "waiting";
    // The rail only knows stages that have rows; the flow state fills the gaps
    // (findings run inside an arena run on Deep and may have no rows yet).
    if (state === "waiting" && at > i) state = "done";
    if (state === "waiting" && at === i) state = "active";
    if (flow.state === "skipped" && i > 0 && own.length === 0) state = "skipped";
    let fill = own.length ? finished / own.length : 0;
    if (state === "done" || state === "failed" || state === "skipped") fill = 1;
    return { stage, label, state, fill };
  });
}

function head(flow: ReviewFlowSnapshot): string {
  switch (flow.state) {
    case "done":
      return "Review done";
    case "cancelled":
      return "Review cancelled";
    case "failed":
      return "Review failed";
    case "skipped":
      return "Review skipped";
    case "awaiting_confirmation":
      return "Deep review needs confirmation";
    default:
      return reviewFlowProgressView(flow)?.headerLabel ?? `Review · ${flow.state}`;
  }
}

function sub(flow: ReviewFlowSnapshot): string {
  if (flow.draining && stateStep(flow.state) < 0) return "Stopping · waiting for running work to finish";
  const tasks = flow.tasks ?? [];
  switch (flow.state) {
    case "triaging":
      return "Choosing review depth and lenses";
    case "awaiting_confirmation":
      return "Triage picked a Deep review. It costs more, so it waits for you.";
    case "finding": {
      const running = tasks.filter((t) => t.stage === "finding" && t.status === "running").length;
      const queued = tasks.filter((t) => t.stage === "finding" && t.status === "queued").length;
      if (!running && !queued) return `${flow.pending_finders.length} finders pending`;
      return `${running} finders running · ${queued} queued for a slot`;
    }
    case "verifying":
      return "Arbiter is checking what the finders proposed";
    case "done":
      return `${flow.kept} kept · ${flow.dropped} dropped`;
    case "failed":
      return flow.reason ?? "Unknown error";
    case "skipped":
      return flow.reason ?? "Triage found nothing that needs a full review";
    default:
      return "";
  }
}

function row(flow: ReviewFlowSnapshot, task: ReviewFlowTaskProgress): RunTaskRow {
  const round =
    task.total_rounds && task.total_rounds > 1 ? `round ${task.round ?? 1}/${task.total_rounds}` : null;
  const model = [task.provider_label, task.model_label].filter(Boolean).join(" · ");
  return {
    task,
    title: task.lens ? lensLabel(task.lens) : task.label,
    round,
    statusLabel: STATUS_LABEL[task.status],
    model,
    dotColor: lensColor(flow, task.lens),
  };
}

/** Everything the jobs popover draws for one review flow. Pure, so the card
 *  can be tested without a DOM. */
export function reviewRunCard(flow: ReviewFlowSnapshot): ReviewRunCard {
  const controls = reviewFlowControls(flow);
  const tasks = flow.tasks ?? [];
  const groups = STAGES.flatMap(({ stage, label }) => {
    const own = tasks.filter((t) => t.stage === stage);
    if (!own.length) return [];
    const note =
      stage === "finding"
        ? [`${own.length} ${own.length === 1 ? "lens" : "lenses"}`, flow.depth].filter(Boolean).join(" · ")
        : null;
    return [{ stage, label, note, rows: own.map((t) => row(flow, t)) }];
  });
  const finders = tasks.filter((t) => t.stage === "finding").length || flow.lenses.length;
  return {
    depth: flow.depth,
    head: head(flow),
    sub: sub(flow),
    steps: steps(flow),
    confirm:
      flow.state === "awaiting_confirmation"
        ? {
            lenses: flow.lenses.map((id) => ({ id, label: lensLabel(id), color: lensColor(flow, id) })),
            estimate: reviewFlowEstimate(flow),
          }
        : null,
    done:
      flow.state === "done"
        ? { kept: flow.kept, dropped: flow.dropped, awaitingAccept: flow.awaiting_accept, finders }
        : null,
    groups,
    openRunId: controls.openRunId ?? (flow.state === "done" ? flow.arena_run_id : null),
    canCancel: controls.cancel,
    canConfirm: controls.confirm,
    canRestart: controls.start,
  };
}

/** A flow worth a card: running, waiting on the user, or finished with
 *  results still waiting to be accepted. */
export function reviewRunVisible(flow: ReviewFlowSnapshot | null | undefined): flow is ReviewFlowSnapshot {
  if (!flow) return false;
  if (reviewFlowControls(flow).busy) return true;
  return flow.state === "done" && flow.awaiting_accept > 0;
}
