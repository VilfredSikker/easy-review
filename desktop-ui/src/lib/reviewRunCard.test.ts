import { describe, expect, it } from "bun:test";
import { lensLabel, reviewRunCard, reviewRunVisible } from "./reviewRunCard";
import type { ReviewFlowSnapshot, ReviewFlowTaskProgress } from "./types";

const flow = (state: ReviewFlowSnapshot["state"], extra: Partial<ReviewFlowSnapshot> = {}): ReviewFlowSnapshot => ({
  state,
  depth: "standard",
  diff_hash: "h",
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
  lens: stage === "finding" ? id : null,
  label: id,
  purpose: "",
  provider_id: "p",
  provider_label: "Provider",
  model_id: "m",
  model_label: "Model",
  status,
  error: null,
  background_task_id: `bg-${id}`,
  round: null,
  total_rounds: null,
  ...extra,
});

describe("reviewRunCard", () => {
  it("marks triage done and findings waiting while a Deep review awaits confirmation", () => {
    const card = reviewRunCard(
      flow("awaiting_confirmation", {
        depth: "deep",
        estimated_cost: 0.82,
        estimated_latency_seconds: 240,
        tasks: [task("triage", "triage", "done")],
      }),
    );
    expect(card.steps.map((s) => s.state)).toEqual(["done", "waiting", "waiting"]);
    expect(card.head).toBe("Deep review needs confirmation");
    expect(card.confirm?.lenses.map((l) => l.label)).toEqual(["General", "Security"]);
    expect(card.confirm?.estimate).toBe("Estimated cost ~$0.82 · time ~240 s");
    expect(card.canConfirm).toBe(true);
    expect(card.canRunStandard).toBe(true);
    expect(card.canCancel).toBe(true);
  });

  it("offers Standard instead only for a pending Deep plan", () => {
    const pending = flow("awaiting_confirmation", { depth: "deep" });
    expect(reviewRunCard({ ...pending, draining: true }).canRunStandard).toBe(false);
    expect(reviewRunCard(flow("finding", { depth: "deep" })).canRunStandard).toBe(false);
    expect(reviewRunCard(flow("awaiting_confirmation", { depth: "standard" })).canRunStandard).toBe(false);
  });

  it("fills a stage by counted rows, never an estimate", () => {
    const card = reviewRunCard(
      flow("finding", {
        tasks: [
          task("triage", "triage", "done"),
          task("general", "finding", "done"),
          task("security", "finding", "running"),
          task("data", "finding", "queued"),
          task("perf", "finding", "failed", { error: "Provider timed out" }),
        ],
      }),
    );
    const findings = card.steps[1];
    expect(findings.state).toBe("active");
    expect(findings.fill).toBe(0.5);
    expect(card.sub).toBe("1 finders running · 1 queued for a slot");
    expect(card.groups.map((g) => g.stage)).toEqual(["triage", "finding"]);
    expect(card.groups[1].note).toBe("4 lenses · standard");
  });

  it("labels rounds and joins provider with model", () => {
    const card = reviewRunCard(
      flow("finding", { tasks: [task("security", "finding", "running", { round: 2, total_rounds: 3 })] }),
    );
    const row = card.groups[0].rows[0];
    expect(row.round).toBe("round 2/3");
    expect(row.model).toBe("Provider · Model");
    expect(row.title).toBe("Security");
    expect(row.dotColor).toBe("var(--color-commit-2)");
  });

  it("summarises a done Deep review that waits for acceptance", () => {
    const card = reviewRunCard(
      flow("done", {
        depth: "deep",
        kept: 4,
        dropped: 2,
        awaiting_accept: 4,
        arena_run_id: "run-1",
        tasks: [task("triage", "triage", "done"), task("verify", "verifying", "done")],
      }),
    );
    expect(card.steps.every((s) => s.state === "done")).toBe(true);
    expect(card.done).toEqual({ kept: 4, dropped: 2, awaitingAccept: 4, finders: 2 });
    expect(card.openRunId).toBe("run-1");
    expect(card.canRestart).toBe(true);
    expect(card.canCancel).toBe(false);
  });

  it("does not allow a restart while the flow drains", () => {
    const card = reviewRunCard(flow("cancelled", { draining: true }));
    expect(card.canRestart).toBe(false);
    expect(card.sub).toBe("Stopping · waiting for running work to finish");
  });
});

describe("reviewRunVisible", () => {
  it("shows busy flows and done flows with findings to accept", () => {
    expect(reviewRunVisible(null)).toBe(false);
    expect(reviewRunVisible(flow("triaging"))).toBe(true);
    expect(reviewRunVisible(flow("done"))).toBe(false);
    expect(reviewRunVisible(flow("done", { awaiting_accept: 2 }))).toBe(true);
    expect(reviewRunVisible(flow("cancelled", { draining: true }))).toBe(true);
  });
});

describe("lensLabel", () => {
  it("turns slugs into words", () => {
    expect(lensLabel("data-model")).toBe("Data model");
    expect(lensLabel("")).toBe("");
  });
});
