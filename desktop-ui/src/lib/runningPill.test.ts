import { describe, expect, it } from "bun:test";
import { otherJobs, reviewRing, runningPillView } from "./runningPill";
import type { RunningJob } from "./runningJobs";
import type { ReviewFlowSnapshot } from "./types";

const job = (id: string, status: RunningJob["status"]): RunningJob => ({
  id,
  source: id.startsWith("arena:") ? "arena" : "background_task",
  label: id,
  status,
  tabIdx: 0,
  startedAtMs: 0,
  finishedAtMs: null,
  error: null,
});

const flow = (state: ReviewFlowSnapshot["state"], extra: Partial<ReviewFlowSnapshot> = {}): ReviewFlowSnapshot => ({
  state,
  depth: "standard",
  diff_hash: "h",
  lenses: [],
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

describe("otherJobs", () => {
  it("drops the flow's own finder tasks and arena run", () => {
    const f = flow("verifying", {
      arena_run_id: "r1",
      tasks: [
        {
          id: "t", stage: "finding", lens: "general", label: "", purpose: "", provider_id: "", provider_label: "",
          model_id: "", model_label: "", status: "done", error: null, background_task_id: "b1", round: null, total_rounds: null,
        },
      ],
    });
    const jobs = [job("task:b1", "running"), job("arena:r1", "running"), job("task:b2", "running")];
    expect(otherJobs(jobs, f).map((j) => j.id)).toEqual(["task:b2"]);
  });
});

describe("runningPillView", () => {
  it("hides when nothing runs", () => {
    expect(runningPillView(null, [])).toBeNull();
    expect(runningPillView(flow("done"), [])).toBeNull();
  });

  it("puts the review first and counts other running work", () => {
    expect(runningPillView(flow("finding"), [job("task:x", "running")])).toEqual({
      tone: "running",
      text: "Review · finding",
      extra: 1,
    });
  });

  it("asks for attention when the flow waits on the user", () => {
    expect(runningPillView(flow("awaiting_confirmation"), [])?.tone).toBe("need");
    expect(runningPillView(flow("done", { awaiting_accept: 3 }), [])?.text).toBe("Review · 3 to accept");
  });

  it("reads a draining flow as stopping", () => {
    expect(runningPillView(flow("cancelled", { draining: true }), [])?.text).toBe("Review · stopping");
  });

  it("settles to done or failed inside the done window", () => {
    expect(runningPillView(null, [job("task:a", "running"), job("task:b", "queued")])?.text).toBe("2 running");
    expect(runningPillView(null, [job("task:a", "done")])?.tone).toBe("done");
    expect(runningPillView(null, [job("task:a", "done"), job("task:b", "failed")])?.tone).toBe("failed");
  });
});

describe("reviewRing", () => {
  it("draws a real share of the circle", () => {
    const r = reviewRing(3, 12);
    expect(r.label).toBe("3/12 reviewed");
    expect(r.arc).toBeCloseTo(r.circumference / 4);
    expect(r.complete).toBe(false);
  });

  it("stays empty with no files and clamps overshoot", () => {
    expect(reviewRing(0, 0).arc).toBe(0);
    expect(reviewRing(0, 0).complete).toBe(false);
    expect(reviewRing(5, 4).label).toBe("4/4 reviewed");
    expect(reviewRing(5, 4).complete).toBe(true);
  });
});
