import { describe, expect, it } from "bun:test";
import { AI_ADVANCED_GROUP, AI_GROUP_ORDER, AI_MENU_ACTION_ORDER, REVIEW_FLOW_ACTION_ID, canRunAdvancedFollowUp, reviewFlowControls, reviewFlowEstimate, reviewFlowProgressView, reviewFlowStage } from "./reviewFlow";
import type { ReviewFlowSnapshot, ReviewFlowTaskProgress } from "./types";

const flow = (state: ReviewFlowSnapshot["state"], extra: Partial<ReviewFlowSnapshot> = {}): ReviewFlowSnapshot => ({
  state,
  depth: "standard",
  diff_hash: "abc123",
  lenses: ["general", "security"],
  pending_finders: [],
  failed_finders: [],
  successful_finder_providers: [],
  successful_arbiter_provider: null,
  providers_used: [],
  arena_run_id: null,
  estimated_cost: null,
  estimated_latency_seconds: null,
  kept: 0,
  dropped: 0,
  awaiting_accept: 0,
  reason: null,
  draining: false,
  ...extra,
});

const task = (
  id: string,
  stage: ReviewFlowTaskProgress["stage"],
  status: ReviewFlowTaskProgress["status"],
  extra: Partial<ReviewFlowTaskProgress> = {},
): ReviewFlowTaskProgress => ({
  id,
  stage,
  lens: stage === "finding" ? "security" : null,
  label: id,
  purpose: `${stage} purpose`,
  provider_id: "openai",
  provider_label: "OpenAI",
  model_id: "gpt-test",
  model_label: "GPT Test",
  status,
  error: null,
  background_task_id: id,
  round: null,
  total_rounds: null,
  ...extra,
});

describe("review flow controls", () => {
  it("uses stable palette identity and keeps Review first", () => {
    expect(REVIEW_FLOW_ACTION_ID).toBe("ai-review-flow");
    expect(AI_GROUP_ORDER).toEqual(["AI", "AI · Advanced"]);
    expect(AI_MENU_ACTION_ORDER).toEqual(["ai-review-flow", "ai-triage", "ai-advanced"]);
    expect(AI_ADVANCED_GROUP).toBe("AI · Advanced");
  });

  it("starts when idle and disables start while active or draining", () => {
    expect(reviewFlowControls(null).start).toBe(true);
    expect(reviewFlowControls(flow("finding")).start).toBe(false);
    expect(reviewFlowControls(flow("failed", { draining: true })).start).toBe(false);
  });

  it("offers Deep decisions only while awaiting confirmation", () => {
    expect(reviewFlowControls(flow("awaiting_confirmation"))).toMatchObject({ confirm: true, decline: true, cancel: true });
    expect(reviewFlowControls(flow("awaiting_confirmation", { draining: true }))).toMatchObject({ confirm: false, decline: false, start: false });
  });

  it("opens only the captured run when acceptance is pending", () => {
    expect(reviewFlowControls(flow("done", { arena_run_id: "run-captured", awaiting_accept: 2 })).openRunId).toBe("run-captured");
    expect(reviewFlowControls(flow("done", { arena_run_id: "run-captured", awaiting_accept: 0 })).openRunId).toBeNull();
    expect(reviewFlowControls(flow("done", { depth: "deep", arena_run_id: "run-deep", awaiting_accept: 0 })).openRunId).toBe("run-deep");
  });

  it("renders backend stages and drain state", () => {
    expect(reviewFlowStage(flow("finding", { pending_finders: ["security"] }))).toBe("Review standard · 1 pending");
    expect(reviewFlowStage(flow("failed", { draining: true, reason: "provider stopped" }))).toBe("Review stopping");
    expect(reviewFlowStage(flow("failed", { successful_arbiter_provider: "openai", providers_used: ["openai"] }))).toBe("Review failed · Unknown error");
    expect(reviewFlowStage(flow("cancelled", { successful_arbiter_provider: "openai", providers_used: ["openai"] }))).toBe("Review cancelled");
    expect(reviewFlowStage(flow("done", { kept: 2, successful_arbiter_provider: "openai", providers_used: ["openai"] }))).toBe("Review done · 2 kept · 0 dropped · single provider · no cross-provider check");
    expect(reviewFlowStage(flow("done", { depth: "deep", awaiting_accept: 3, successful_arbiter_provider: "openai", providers_used: ["openai"] }))).toBe("Review done · 3 findings waiting to accept · single provider · no cross-provider check");
    expect(reviewFlowStage(flow("done", { depth: "deep", awaiting_accept: 0, successful_arbiter_provider: null, providers_used: [] }))).toBe("Review done");
    expect(reviewFlowStage(flow("done", { depth: "quick", awaiting_accept: 1, kept: 2, dropped: 1, successful_arbiter_provider: "openai", providers_used: ["openai", "anthropic"] }))).toBe("Review done · 2 kept · 1 dropped");
    expect(reviewFlowStage(flow("done", { depth: "standard", kept: 2, successful_arbiter_provider: null, providers_used: [] }))).toBe("Review done · 2 kept · 0 dropped");
  });

  it("does not promise a complete plan while triage is still running", () => {
    const progress = reviewFlowProgressView(flow("triaging", {
      tasks: [task("triage", "triage", "running")],
    }));
    expect(progress).toMatchObject({ activeStage: "triage", showCounts: false, remaining: 1 });
  });

  it("shows standard partial failure and queued work from captured tasks", () => {
    const progress = reviewFlowProgressView(flow("finding", {
      tasks: [
        task("security", "finding", "running"),
        task("database", "finding", "failed", { error: "provider timed out" }),
        task("reliability", "finding", "queued"),
      ],
    }));
    expect(progress).toMatchObject({
      activeStage: "finding",
      finished: 1,
      remaining: 2,
      showCounts: true,
    });
    expect(progress?.visibleTasks.map(({ status }) => status)).toEqual(["running", "failed", "queued"]);
    expect(progress?.visibleTasks[1].error).toBe("provider timed out");
  });

  it("uses an active deep cross-check row instead of the reducer phase", () => {
    const progress = reviewFlowProgressView(flow("verifying", {
      tasks: [
        task("finding-r1", "finding", "done", { round: 1, total_rounds: 2 }),
        task("verify-r2", "verifying", "running", {
          round: 2,
          total_rounds: 2,
          label: "Crosscheck R2",
          purpose: "Cross-check round 2",
        }),
      ],
    }));
    expect(progress?.activeStage).toBe("verifying");
    expect(progress?.activeGoal).toBe("Cross-check round 2");
    expect(reviewFlowStage(flow("verifying", { tasks: [task("verify", "verifying", "running")] }))).toBe("Review Verification");
    expect(progress?.activeStageLabel).toBe("Verification");
  });

  it("keeps terminal, draining, and confirmation copy ahead of stale task rows", () => {
    const running = task("stale", "finding", "running");
    const draining = reviewFlowProgressView(flow("cancelled", { draining: true, tasks: [running] }));
    expect(draining).toMatchObject({
      headerLabel: "Review stopping",
      activeStage: null,
      activeGoal: "Review stopping",
      rail: expect.arrayContaining([expect.objectContaining({ stage: "finding", state: "active" })]),
      visibleTasks: [running],
    });
    expect(reviewFlowStage(flow("cancelled", { draining: true, tasks: [running] }))).toBe("Review stopping");

    const failed = reviewFlowProgressView(flow("failed", { reason: "provider failed", tasks: [running] }));
    expect(failed).toMatchObject({ headerLabel: "Review failed · provider failed", activeStage: null });
    expect(reviewFlowStage(flow("failed", { reason: "provider failed", tasks: [running] }))).toBe("Review failed · provider failed");

    const confirming = reviewFlowProgressView(flow("awaiting_confirmation", {
      tasks: [task("queued", "finding", "queued")],
    }));
    expect(confirming).toMatchObject({
      headerLabel: "Deep review needs confirmation",
      activeStage: null,
      activeGoal: "Waiting for your confirmation",
      rail: expect.arrayContaining([expect.objectContaining({ stage: "finding", state: "waiting" })]),
    });
    expect(reviewFlowStage(flow("awaiting_confirmation", { tasks: [task("queued", "finding", "queued")] }))).toBe("Deep review needs confirmation");

    const done = reviewFlowProgressView(flow("done", {
      kept: 1,
      dropped: 2,
      tasks: [task("stale-queued", "finding", "queued")],
    }));
    expect(done).toMatchObject({
      headerLabel: "Review done · 1 kept · 2 dropped",
      activeStage: null,
      rail: expect.arrayContaining([expect.objectContaining({ stage: "finding", state: "waiting" })]),
    });
  });

  it("keeps terminal task summaries available without active rows", () => {
    const done = flow("done", { kept: 2, tasks: [task("triage", "triage", "done"), task("find", "finding", "done")] });
    const progress = reviewFlowProgressView(done);
    expect(progress).toMatchObject({ finished: 2, remaining: 0, showCounts: true, visibleTasks: [] });
    expect(reviewFlowStage(done)).toBe("Review done · 2 kept · 0 dropped");
  });

  it("shows verification as skipped when the arbiter did not run", () => {
    const progress = reviewFlowProgressView(flow("done", {
      tasks: [task("triage", "triage", "done"), task("find", "finding", "done"), task("arbiter", "verifying", "skipped")],
    }));
    expect(progress?.rail).toContainEqual({ stage: "verifying", label: "Verification", state: "skipped" });
  });

  it("shows the captured Deep estimate and keeps decline available before confirmation", () => {
    const deep = flow("awaiting_confirmation", { estimated_cost: 0.18, estimated_latency_seconds: 135 });
    expect(reviewFlowEstimate(deep)).toBe("Estimated cost ~$0.18 · time ~135 s");
    expect(reviewFlowControls(deep)).toMatchObject({ confirm: true, decline: true });
  });

  it("blocks the Advanced triage follow-up while a flow or another follow-up is active", () => {
    expect(canRunAdvancedFollowUp(null, false)).toBe(true);
    expect(canRunAdvancedFollowUp(flow("finding"), false)).toBe(false);
    expect(canRunAdvancedFollowUp(null, true)).toBe(false);
  });
});
