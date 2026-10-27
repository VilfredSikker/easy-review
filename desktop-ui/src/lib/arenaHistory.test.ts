import { describe, expect, it } from "bun:test";
import { captureHistoryFlowOrigin, openArenaHistory } from "./arenaHistory";
import { arenaAcceptanceRoute } from "./arenaFlowAcceptance";
import type { ReviewFlowSnapshot } from "./types";
import type { ArenaRunSnapshot } from "./types/arena";

function snapshot(kind: "models" | "review_flow" | "seeded"): ArenaRunSnapshot {
  return {
    run: { id: "deep-a", config: { run_kind: kind }, reviewers: [{}, {}] },
  } as ArenaRunSnapshot;
}

function capturedFlow(depth: ReviewFlowSnapshot["depth"]) {
  return captureHistoryFlowOrigin({
    review_bucket_er_dir: "/a/branch",
    review_flow: { depth, arena_run_id: "deep-a" } as ReviewFlowSnapshot,
  });
}

describe("opening Arena history", () => {
  it("captures only Deep flow ownership from the visible bucket", () => {
    expect(capturedFlow("deep")).toEqual({ erDir: "/a/branch", runId: "deep-a" });
    expect(captureHistoryFlowOrigin(null)).toBeNull();
  });

  it("keeps Quick and Standard seeded history on the verification path", async () => {
    for (const depth of ["quick", "standard"] as const) {
      const origin = capturedFlow(depth);
      expect(origin).toBeNull();
      const calls: string[] = [];
      const opened = await openArenaHistory("deep-a", origin, {
        loadArena: async () => { calls.push("arena_get"); return snapshot("seeded"); },
        loadFlow: async () => { throw new Error("Seeded verification is not a Deep result"); },
      });
      expect(calls).toEqual(["arena_get"]);
      expect(opened.snapshot.run.config.run_kind).toBe("seeded");
      expect(opened.flowOrigin).toBeNull();
    }
  });
  it("restores the captured bucket before accepting a current Deep result", async () => {
    const calls: string[] = [];
    const opened = await openArenaHistory("deep-a", capturedFlow("deep"), {
      loadArena: async () => { calls.push("arena_get"); return snapshot("review_flow"); },
      loadFlow: async (origin) => { calls.push(`flow_get_run:${origin.erDir}:${origin.runId}`); return snapshot("review_flow"); },
    });
    expect(calls).toEqual(["flow_get_run:/a/branch:deep-a"]);
    expect(arenaAcceptanceRoute(opened.flowOrigin, "deep-a", ["f1"], opened.snapshot.run.config.run_kind)).toEqual({
      kind: "flow", erDir: "/a/branch", runId: "deep-a", findingIds: ["f1"],
    });
  });

  it("does not treat a former Deep run as the current flow or allow manual import", async () => {
    const opened = await openArenaHistory("deep-a", { erDir: "/a/branch", runId: "deep-b" }, {
      loadArena: async () => snapshot("review_flow"),
      loadFlow: async () => { throw new Error("must not load the new flow"); },
    });
    expect(opened.snapshot.run.id).toBe("deep-a");
    expect(opened.flowOrigin).toBeNull();
    expect(() => arenaAcceptanceRoute(opened.flowOrigin, "deep-a", ["f1"], opened.snapshot.run.config.run_kind)).toThrow("Start a new Review");
  });

  it("keeps historical flow results viewable after restart without importing them", async () => {
    const opened = await openArenaHistory("deep-a", null, {
      loadArena: async () => snapshot("review_flow"),
      loadFlow: async () => { throw new Error("no captured flow exists"); },
    });
    expect(opened.flowOrigin).toBeNull();
    expect(() => arenaAcceptanceRoute(null, "deep-a", ["f1"], opened.snapshot.run.config.run_kind)).toThrow("Start a new Review");
  });

  it("keeps manual history on the manual acceptance command", async () => {
    const opened = await openArenaHistory("deep-a", null, {
      loadArena: async () => snapshot("models"),
      loadFlow: async () => { throw new Error("manual run has no flow"); },
    });
    expect(arenaAcceptanceRoute(opened.flowOrigin, "deep-a", ["f1"], opened.snapshot.run.config.run_kind).kind).toBe("manual");
  });
});
