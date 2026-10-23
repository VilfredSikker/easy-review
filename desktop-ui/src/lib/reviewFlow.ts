import type { ReviewFlowSnapshot, ReviewFlowTaskProgress } from "$lib/types";

export const REVIEW_FLOW_ACTION_ID = "ai-review-flow";
export const AI_ADVANCED_GROUP = "AI · Advanced" as const;

export type ReviewFlowControls = {
  start: boolean;
  cancel: boolean;
  confirm: boolean;
  decline: boolean;
  openRunId: string | null;
  busy: boolean;
};

export type ReviewFlowProgressStage = ReviewFlowTaskProgress["stage"];
export type ReviewFlowStageRailState = "waiting" | "active" | "done" | "failed" | "skipped";

export interface ReviewFlowProgressView {
  headerLabel: string;
  activeStage: ReviewFlowProgressStage | null;
  activeStageLabel: string | null;
  activeGoal: string;
  rail: { stage: ReviewFlowProgressStage; label: string; state: ReviewFlowStageRailState }[];
  visibleTasks: ReviewFlowTaskProgress[];
  finished: number;
  remaining: number;
  showCounts: boolean;
}

const FLOW_STAGES: { stage: ReviewFlowProgressStage; label: string; goal: string }[] = [
  { stage: "triage", label: "Triage", goal: "Choosing the review plan" },
  { stage: "finding", label: "Findings", goal: "Reviewing with the assigned models" },
  { stage: "verifying", label: "Verification", goal: "Cross-checking the findings" },
];

/** Project only captured task rows. Before triage completes, its plan may grow. */
export function reviewFlowProgressView(
  flow: ReviewFlowSnapshot,
): ReviewFlowProgressView | null {
  const tasks = flow.tasks ?? [];
  if (tasks.length === 0) return null;

  const running = tasks.filter((task) => task.status === "running");
  const queued = tasks.filter((task) => task.status === "queued");
  const stageOrder = new Map(FLOW_STAGES.map(({ stage }, index) => [stage, index]));
  const current = (running.length > 0 ? running : queued).reduce<ReviewFlowTaskProgress | null>(
    (latest, task) =>
      latest == null || (stageOrder.get(task.stage) ?? -1) > (stageOrder.get(latest.stage) ?? -1)
        ? task
        : latest,
    null,
  );
  const stateUsesTaskStage = ["triaging", "finding", "verifying"].includes(flow.state);
  const activeStage = stateUsesTaskStage ? current?.stage ?? null : null;
  const stageCanBeActive = stateUsesTaskStage || flow.draining;
  const rail = FLOW_STAGES.map(({ stage, label }) => {
    const stageTasks = tasks.filter((task) => task.stage === stage);
    const hasPending = stageCanBeActive && stageTasks.some((task) =>
      task.status === "queued" || task.status === "running",
    );
    const completedRows = stageTasks.filter((task) =>
      ["done", "failed", "cancelled", "skipped"].includes(task.status),
    );
    const hasFailed = stageTasks.some((task) => task.status === "failed");
    const complete = stageTasks.length > 0 && completedRows.length === stageTasks.length;
    let state: ReviewFlowStageRailState = "waiting";
    if (hasPending) state = "active";
    else if (complete) {
      if (hasFailed) state = "failed";
      else state = stageTasks.every((task) => task.status === "skipped" || task.status === "cancelled") ? "skipped" : "done";
    }
    return { stage, label, state };
  });
  const finished = tasks.filter((task) =>
    ["done", "failed", "cancelled", "skipped"].includes(task.status),
  ).length;
  const remaining = tasks.length - finished;
  const visibleTasks = tasks.filter((task) =>
    ["running", "queued", "failed"].includes(task.status),
  );
  const goal = FLOW_STAGES.find(({ stage }) => stage === activeStage)?.goal;
  const overall = reviewFlowOverallStage(flow);
  let activeGoal = overall;
  if (activeStage) activeGoal = current?.purpose || goal || "Review process";
  else if (flow.state === "awaiting_confirmation") activeGoal = "Waiting for your confirmation";

  return {
    headerLabel: activeStage
      ? `Review · ${FLOW_STAGES.find(({ stage }) => stage === activeStage)?.label}`
      : overall,
    activeStage,
    activeStageLabel: FLOW_STAGES.find(({ stage }) => stage === activeStage)?.label ?? null,
    activeGoal,
    rail,
    visibleTasks,
    finished,
    remaining,
    showCounts: flow.state !== "triaging",
  };
}

/** Derive available actions from backend state. Draining keeps the bucket busy
 * even after the visible flow reached a terminal state. */
export function reviewFlowControls(
  flow: ReviewFlowSnapshot | null | undefined,
): ReviewFlowControls {
  const state = flow?.state ?? null;
  const active = state === "triaging" || state === "finding" || state === "verifying" || state === "awaiting_confirmation";
  const busy = active || Boolean(flow?.draining);
  return {
    start: !busy,
    cancel: active,
    confirm: state === "awaiting_confirmation" && !flow?.draining,
    decline: state === "awaiting_confirmation" && !flow?.draining,
    openRunId: state === "done" && ((flow?.awaiting_accept ?? 0) > 0 || flow?.depth === "deep")
      ? flow?.arena_run_id ?? null
      : null,
    busy,
  };
}

export function reviewFlowStage(flow: ReviewFlowSnapshot): string {
  const progress = reviewFlowProgressView(flow);
  if (progress?.activeStageLabel) return `Review ${progress.activeStageLabel}`;
  return reviewFlowOverallStage(flow);
}

function reviewFlowOverallStage(flow: ReviewFlowSnapshot): string {
  if (flow.draining && !["triaging", "finding", "verifying", "awaiting_confirmation"].includes(flow.state)) {
    return "Review stopping";
  }
  if (flow.state === "finding") return `Review ${flow.depth ?? ""} · ${flow.pending_finders.length} pending`.trim();
  if (flow.state === "awaiting_confirmation") return "Deep review needs confirmation";
  if (flow.state === "done") {
    const providerNote = flow.successful_arbiter_provider && flow.providers_used.length < 2
      ? " · single provider · no cross-provider check"
      : "";
    if (flow.depth === "deep") {
      if (flow.awaiting_accept === 0) return `Review done${providerNote}`;
      const finding = flow.awaiting_accept === 1 ? "finding" : "findings";
      return `Review done · ${flow.awaiting_accept} ${finding} waiting to accept${providerNote}`;
    }
    return `Review done · ${flow.kept} kept · ${flow.dropped} dropped${providerNote}`;
  }
  if (flow.state === "failed") return `Review failed · ${flow.reason ?? "Unknown error"}`;
  return ({ triaging: "Review triaging", verifying: "Review verifying", skipped: "Review skipped", cancelled: "Review cancelled" } as const)[flow.state];
}

export function reviewFlowEstimate(flow: ReviewFlowSnapshot | null | undefined): string | null {
  if (flow?.state !== "awaiting_confirmation") return null;
  const cost = flow.estimated_cost == null ? "unavailable" : `~$${flow.estimated_cost.toFixed(2)}`;
  const latency = flow.estimated_latency_seconds == null ? "unavailable" : `~${flow.estimated_latency_seconds} s`;
  return `Estimated cost ${cost} · time ${latency}`;
}

export function canRunAdvancedFollowUp(
  flow: ReviewFlowSnapshot | null | undefined,
  pending: boolean,
): boolean {
  return reviewFlowControls(flow).start && !pending;
}

export const AI_GROUP_ORDER = ["AI", AI_ADVANCED_GROUP] as const;
export const AI_MENU_ACTION_ORDER = [REVIEW_FLOW_ACTION_ID, "ai-triage", "ai-advanced"] as const;
