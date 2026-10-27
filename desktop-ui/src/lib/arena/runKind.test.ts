import { describe, expect, it } from "bun:test";
import type { ArenaRunSnapshot, Reviewer, RunStatus } from "$lib/types/arena";
import { performUnlessSeeded } from "./runAction";
import {
  isArenaRunFromSnapshot,
  isSeededRunFromSnapshot,
  isSingleReviewRun,
  seededVerificationNotice,
} from "./runKind";

function snapshot(
  runKind: "seeded" | "models" | "agent" | undefined,
  reviewerCount: number,
  status: RunStatus = "complete",
): ArenaRunSnapshot {
  return {
    run: {
      status,
      config: { run_kind: runKind },
      reviewers: Array.from({ length: reviewerCount }, (_, index) => ({ id: `r${index}` })) as Reviewer[],
    },
  } as unknown as ArenaRunSnapshot;
}

describe("run kind classification", () => {
  it("routes seeded verification with zero, one, or many raisers to a read-only overlay", () => {
    for (const count of [0, 1, 3]) {
      const snap = snapshot("seeded", count);
      expect(isSeededRunFromSnapshot(snap)).toBe(true);
      expect(isArenaRunFromSnapshot(snap)).toBe(true);
      expect(isSingleReviewRun(null, snap)).toBe(false);
    }
  });

  it("keeps legacy, model, and agent one-reviewer runs importable", () => {
    for (const kind of [undefined, "models", "agent"] as const) {
      const snap = snapshot(kind, 1);
      expect(isSeededRunFromSnapshot(snap)).toBe(false);
      expect(isSingleReviewRun(null, snap)).toBe(true);
      expect(isArenaRunFromSnapshot(snap)).toBe(false);
    }
  });

  it("keeps manual arenas and Deep flow arenas on the acceptance overlay path", () => {
    expect(isArenaRunFromSnapshot(snapshot(undefined, 2))).toBe(true);
    expect(isArenaRunFromSnapshot(snapshot("models", 2))).toBe(true);
    expect(isSingleReviewRun(null, snapshot("models", 2))).toBe(false);
  });
});

describe("seeded verification actions", () => {
  it("does not call the import action for a one-raiser seeded run", async () => {
    let imports = 0;
    const result = await performUnlessSeeded(snapshot("seeded", 1), async () => {
      imports += 1;
      return 1;
    });
    expect(result).toEqual({ kind: "readOnlyVerification" });
    expect(imports).toBe(0);
  });

  it("still performs acceptance for a manual arena", async () => {
    const result = await performUnlessSeeded(snapshot("models", 2), async () => 2);
    expect(result).toEqual({ kind: "performed", value: 2 });
  });

  it("uses status-specific notices without claiming failed or pending results were applied", () => {
    const complete = snapshot("seeded", 1, "complete");
    expect(seededVerificationNotice(complete)).toBe(
      "Verification complete. Results apply automatically to the matching review.",
    );
    expect(seededVerificationNotice(snapshot("seeded", 1, "queued"))).toBe(
      "Verification is running. Results appear in Review when it completes.",
    );
    expect(seededVerificationNotice(snapshot("seeded", 1, { running: { round: 1 } }))).toBe(
      "Verification is running. Results appear in Review when it completes.",
    );
    expect(seededVerificationNotice(snapshot("seeded", 1, "failed"))).toBe(
      "Verification failed. Check the run errors.",
    );
    expect(seededVerificationNotice(snapshot("seeded", 1, "cancelled"))).toBe(
      "Verification cancelled.",
    );
  });

  it("keeps seeded runs read-only across completion, failure, cancellation, and active states", async () => {
    for (const status of ["complete", "failed", "cancelled", "queued", { running: { round: 1 } }] as const) {
      const snap = snapshot("seeded", 1, status);
      let actions = 0;
      expect(isSeededRunFromSnapshot(snap)).toBe(true);
      expect(await performUnlessSeeded(snap, async () => ++actions)).toEqual({ kind: "readOnlyVerification" });
      expect(actions).toBe(0);
    }
  });
});
